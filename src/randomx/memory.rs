extern crate argon2;

use std::arch::x86_64::{_mm_prefetch, _MM_HINT_NTA};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use argon2::Block;

use super::super::byte_string;
use super::superscalar::{Blake2Generator, ScProgram};

const RANDOMX_ARGON_LANES: u32 = 1;
const RANDOMX_ARGON_MEMORY: u32 = 262144;
const RANDOMX_ARGON_SALT: &[u8; 8] = b"RandomX\x03";
const RANDOMX_ARGON_ITERATIONS: u32 = 3;
const RANDOMX_CACHE_ACCESSES: usize = 8;

const ARGON2_SYNC_POINTS: u32 = 4;
const ARGON_BLOCK_SIZE: u32 = 1024;

pub const CACHE_LINE_SIZE: u64 = 64;
pub const DATASET_ITEM_COUNT: usize = (2147483648 + 33554368) / 64; //34.078.719

const SUPERSCALAR_MUL_0: u64 = 6364136223846793005;
const SUPERSCALAR_ADD_1: u64 = 9298411001130361340;
const SUPERSCALAR_ADD_2: u64 = 12065312585734608966;
const SUPERSCALAR_ADD_3: u64 = 9306329213124626780;
const SUPERSCALAR_ADD_4: u64 = 5281919268842080866;
const SUPERSCALAR_ADD_5: u64 = 10536153434571861004;
const SUPERSCALAR_ADD_6: u64 = 3398623926847679864;
const SUPERSCALAR_ADD_7: u64 = 9549104520008361294;

//256MiB, always used, named randomx_cache in the reference implementation
pub struct SeedMemory {
    pub blocks: Box<[Block]>,
    pub programs: Vec<ScProgram<'static>>,
}

impl SeedMemory {
    pub fn no_memory() -> SeedMemory {
        SeedMemory {
            blocks: Box::new([]),
            programs: Vec::with_capacity(0),
        }
    }

    /// Creates a new initialised seed memory.
    pub fn new_initialised(key: &[u8]) -> SeedMemory {
        let mut mem = argon2::Memory::new(RANDOMX_ARGON_LANES, RANDOMX_ARGON_MEMORY);
        let context = &create_argon_context(key);
        argon2::initialize(context, &mut mem);
        argon2::fill_memory_blocks(context, &mut mem);

        let mut programs = Vec::with_capacity(RANDOMX_CACHE_ACCESSES);
        let mut gen = Blake2Generator::new(key, 0);
        for _ in 0..RANDOMX_CACHE_ACCESSES {
            programs.push(ScProgram::generate(&mut gen));
        }

        SeedMemory {
            blocks: mem.blocks,
            programs,
        }
    }
}

fn create_argon_context(key: &[u8]) -> argon2::Context {
    let segment_length = RANDOMX_ARGON_MEMORY / (RANDOMX_ARGON_LANES * ARGON2_SYNC_POINTS);
    let config = argon2::Config {
        ad: &[],
        hash_length: 0,
        lanes: RANDOMX_ARGON_LANES,
        mem_cost: RANDOMX_ARGON_MEMORY,
        secret: &[],
        time_cost: RANDOMX_ARGON_ITERATIONS,
        variant: argon2::Variant::Argon2d,
        version: argon2::Version::Version13,
    };
    //TODO do i need all the params it had b4?
    argon2::Context {
        config,
        memory_blocks: RANDOMX_ARGON_MEMORY,
        pwd: key,
        salt: RANDOMX_ARGON_SALT,
        lane_length: segment_length * ARGON2_SYNC_POINTS,
        segment_length,
    }
}

fn mix_block_value(seed_mem: &SeedMemory, reg_value: u64, r: usize) -> u64 {
    let mask = (((RANDOMX_ARGON_MEMORY * ARGON_BLOCK_SIZE) as u64) / CACHE_LINE_SIZE) - 1;
    let byte_offset = ((reg_value & mask) * CACHE_LINE_SIZE) + (8 * r as u64);

    let block_ix = byte_offset / ARGON_BLOCK_SIZE as u64;
    let block_v_ix = (byte_offset - (block_ix * ARGON_BLOCK_SIZE as u64)) / 8;
    seed_mem.blocks[block_ix as usize][block_v_ix as usize]
}

pub fn init_dataset_item(seed_mem: &SeedMemory, item_num: u64) -> [u64; 8] {
    let mut ds = [0; 8];

    let mut reg_value = item_num;
    ds[0] = (item_num + 1).wrapping_mul(SUPERSCALAR_MUL_0);
    ds[1] = ds[0] ^ SUPERSCALAR_ADD_1;
    ds[2] = ds[0] ^ SUPERSCALAR_ADD_2;
    ds[3] = ds[0] ^ SUPERSCALAR_ADD_3;
    ds[4] = ds[0] ^ SUPERSCALAR_ADD_4;
    ds[5] = ds[0] ^ SUPERSCALAR_ADD_5;
    ds[6] = ds[0] ^ SUPERSCALAR_ADD_6;
    ds[7] = ds[0] ^ SUPERSCALAR_ADD_7;

    for prog in &seed_mem.programs {
        prog.execute(&mut ds);

        for (r, v) in ds.iter_mut().enumerate() {
            let mix_value = mix_block_value(seed_mem, reg_value, r);
            *v ^= mix_value;
        }
        reg_value = ds[prog.address_reg];
    }
    ds
}

#[derive(Clone)]
pub struct VmMemoryAllocator {
    pub vm_memory_seed: String,
    pub vm_memory: Arc<VmMemory>,
}

impl VmMemoryAllocator {
    pub fn initial() -> VmMemoryAllocator {
        VmMemoryAllocator {
            vm_memory_seed: "".to_string(),
            vm_memory: Arc::new(VmMemory::no_memory()),
        }
    }

    pub fn reallocate(&mut self, seed: String) -> bool {
        if seed != self.vm_memory_seed {
            let mem_init_start = Instant::now();
            self.vm_memory = Arc::new(VmMemory::full(&byte_string::string_to_u8_array(&seed)));
            self.vm_memory_seed = seed;
            info!(
                "memory init took {}ms with seed_hash: {}",
                mem_init_start.elapsed().as_millis(),
                self.vm_memory_seed,
            );
            return true; // Memory was reallocated
        }
        false // No reallocation needed
    }
    
    // Add get_memory method to retrieve the current memory Arc
    pub fn get_memory(&self) -> Arc<VmMemory> {
        self.vm_memory.clone()
    }
}

pub struct VmMemory {
    pub seed_memory: SeedMemory,
    pub dataset_memory: RwLock<Vec<Option<[u64; 8]>>>,
    pub cache: bool,
}

impl VmMemory {
    //only useful for testing
    pub fn no_memory() -> VmMemory {
        VmMemory {
            seed_memory: SeedMemory::no_memory(),
            cache: false,
            dataset_memory: RwLock::new(Vec::with_capacity(0)),
        }
    }

    pub fn light(key: &[u8]) -> VmMemory {
        VmMemory {
            seed_memory: SeedMemory::new_initialised(key),
            cache: false,
            dataset_memory: RwLock::new(Vec::with_capacity(0)),
        }
    }
    pub fn full(key: &[u8]) -> VmMemory {
        let seed_mem = SeedMemory::new_initialised(key);
        let mem = vec![None; DATASET_ITEM_COUNT];
        VmMemory {
            seed_memory: seed_mem,
            cache: true,
            dataset_memory: RwLock::new(mem),
        }
    }

    pub fn dataset_prefetch(&self, offset: u64) {
        if !self.cache {
            return; // Skip prefetching for non-cached memory
        }

        let item_num = offset / CACHE_LINE_SIZE;

        // Quick read lock to check if the item is cached
        let need_init = {
            let mem = self.dataset_memory.read().unwrap();
            let rl_cached = &mem[item_num as usize];

            if let Some(rl) = rl_cached {
                // Item exists in cache, prefetch it
                unsafe {
                    let raw: *const i8 = std::mem::transmute(rl);
                    _mm_prefetch(raw, _MM_HINT_NTA);
                }
                false
            } else {
                // Item doesn't exist in cache
                true
            }
        };

        // prefetch the next few items as well (spatial locality)
        if need_init && item_num + 1 < DATASET_ITEM_COUNT as u64 {
            // Precompute the next item asynchronously if it's not in cache
            // We don't actually need to do anything here as the next read will initialize it
            // This is just a hint to the code that we might need it soon
        }
    }

    pub fn dataset_read(&self, offset: u64, reg: &mut [u64; 8]) {
        let item_num = offset / CACHE_LINE_SIZE;

        if self.cache {
            // Use a scope for the read lock to ensure it's dropped quickly
            let rl_opt: std::option::Option<[u64; 8]> = {
                let mem = self.dataset_memory.read().unwrap();
                let rl_cached = &mem[item_num as usize];
                if let Some(rl) = rl_cached {
                    // If cached, apply XOR directly and return
                    reg[0] ^= rl[0];
                    reg[1] ^= rl[1];
                    reg[2] ^= rl[2];
                    reg[3] ^= rl[3];
                    reg[4] ^= rl[4];
                    reg[5] ^= rl[5];
                    reg[6] ^= rl[6];
                    reg[7] ^= rl[7];
                    return;
                }
                None
            };

            // If we get here, we need to initialize the item
            if rl_opt.is_none() {
                let rl = init_dataset_item(&self.seed_memory, item_num);

                // Apply XOR
                reg[0] ^= rl[0];
                reg[1] ^= rl[1];
                reg[2] ^= rl[2];
                reg[3] ^= rl[3];
                reg[4] ^= rl[4];
                reg[5] ^= rl[5];
                reg[6] ^= rl[6];
                reg[7] ^= rl[7];

                // Cache the result after applying XOR
                let mut mem_mut = self.dataset_memory.write().unwrap();
                mem_mut[item_num as usize] = Some(rl);
            }
        } else {
            // Non-cached version
            let rl = init_dataset_item(&self.seed_memory, item_num);

            // Unrolled loop for better performance
            reg[0] ^= rl[0];
            reg[1] ^= rl[1];
            reg[2] ^= rl[2];
            reg[3] ^= rl[3];
            reg[4] ^= rl[4];
            reg[5] ^= rl[5];
            reg[6] ^= rl[6];
            reg[7] ^= rl[7];
        }
    }
}
