use super::m128::m128i;

#[allow(overflowing_literals)]
fn keys_1rx4() -> (m128i, m128i, m128i, m128i) {
    (
        m128i::from_i32(0xb4f44917, 0xdbb5552b, 0x62716609, 0x6daca553),
        m128i::from_i32(0x0da1dc4e, 0x1725d378, 0x846a710d, 0x6d7caf07),
        m128i::from_i32(0x3e20e345, 0xf4c0794f, 0x9f947ec6, 0x3f1262f1),
        m128i::from_i32(0x49169154, 0x16314c88, 0xb1ba317c, 0x6aef8135),
    )
}

#[allow(overflowing_literals)]
pub fn hash_aes_1rx4(input: &[u64]) -> [m128i; 4] {
    debug_assert!(input.len() % 64 == 0);

    // Initialize state with constants
    let mut state0 = m128i::from_i32(0xd7983aad, 0xcc82db47, 0x9fa856de, 0x92b52c0d);
    let mut state1 = m128i::from_i32(0xace78057, 0xf59e125a, 0x15c7b798, 0x338d996e);
    let mut state2 = m128i::from_i32(0xe8a07ce4, 0x5079506b, 0xae62c7d0, 0x6a770017);
    let mut state3 = m128i::from_i32(0x7e994948, 0x79a10005, 0x07ad828d, 0x630a240c);

    // Pre-calculate the chunks size for better prediction
    let chunks = input.len() / 8;
    
    // Process input in chunks of 8 u64 values
    for chunk in 0..chunks {
        let base_idx = chunk * 8;
        
        // Load input values for this chunk
        let in0 = m128i::from_u64(input[base_idx + 1], input[base_idx]);
        let in1 = m128i::from_u64(input[base_idx + 3], input[base_idx + 2]);
        let in2 = m128i::from_u64(input[base_idx + 5], input[base_idx + 4]);
        let in3 = m128i::from_u64(input[base_idx + 7], input[base_idx + 6]);

        // Update states with AES operations
        state0 = state0.aesenc(in0);
        state1 = state1.aesdec(in1);
        state2 = state2.aesenc(in2);
        state3 = state3.aesdec(in3);
    }

    // Final mixing with constant keys
    let x_key_0 = m128i::from_i32(0x06890201, 0x90dc56bf, 0x8b24949f, 0xf6fa8389);
    let x_key_1 = m128i::from_i32(0xed18f99b, 0xee1043c6, 0x51f4e03c, 0x61b263d1);

    // Apply final rounds of AES encryption/decryption
    state0 = state0.aesenc(x_key_0);
    state1 = state1.aesdec(x_key_0);
    state2 = state2.aesenc(x_key_0);
    state3 = state3.aesdec(x_key_0);

    state0 = state0.aesenc(x_key_1);
    state1 = state1.aesdec(x_key_1);
    state2 = state2.aesenc(x_key_1);
    state3 = state3.aesdec(x_key_1);

    [state0, state1, state2, state3]
}

pub fn fill_aes_1rx4_u64(input: &[m128i; 4], into: &mut Vec<u64>) -> [m128i; 4] {
    // Get the AES keys once
    let (key0, key1, key2, key3) = keys_1rx4();
    
    // Copy input states
    let mut state0 = input[0];
    let mut state1 = input[1];
    let mut state2 = input[2];
    let mut state3 = input[3];

    // Calculate how many chunks we need to process
    let chunks = into.len() / 8;
    
    // Process each chunk of 8 elements
    for chunk in 0..chunks {
        // Calculate output index for this chunk
        let out_ix = chunk * 8;
        
        // Apply AES operations to states
        state0 = state0.aesdec(key0);
        state1 = state1.aesenc(key1);
        state2 = state2.aesdec(key2);
        state3 = state3.aesenc(key3);
        
        // Extract results from states
        let (s0_1, s0_0) = state0.as_i64();
        let (s1_1, s1_0) = state1.as_i64();
        let (s2_1, s2_0) = state2.as_i64();
        let (s3_1, s3_0) = state3.as_i64();
        
        // Store results in output vector
        into[out_ix]     = s0_0 as u64;
        into[out_ix + 1] = s0_1 as u64;
        into[out_ix + 2] = s1_0 as u64;
        into[out_ix + 3] = s1_1 as u64;
        into[out_ix + 4] = s2_0 as u64;
        into[out_ix + 5] = s2_1 as u64;
        into[out_ix + 6] = s3_0 as u64;
        into[out_ix + 7] = s3_1 as u64;
    }
    
    // Return final state
    [state0, state1, state2, state3]
}

fn fill_aes_1rx4_m128i(input: &[m128i; 4], into: &mut Vec<m128i>) -> [m128i; 4] {
    // Get AES keys
    let (key0, key1, key2, key3) = keys_1rx4();
    
    // Copy input states
    let mut state0 = input[0];
    let mut state1 = input[1];
    let mut state2 = input[2];
    let mut state3 = input[3];
    
    // Calculate the number of chunks to process
    let chunks = into.len() / 4;
    
    // Process each chunk
    for chunk in 0..chunks {
        let out_ix = chunk * 4;
        
        // Apply AES operations to states
        state0 = state0.aesdec(key0);
        state1 = state1.aesenc(key1);
        state2 = state2.aesdec(key2);
        state3 = state3.aesenc(key3);
        
        // Store results directly
        into[out_ix] = state0;
        into[out_ix + 1] = state1;
        into[out_ix + 2] = state2;
        into[out_ix + 3] = state3;
    }
    
    // Return final state
    [state0, state1, state2, state3]
}

pub fn gen_program_aes_1rx4(input: &[m128i; 4], output_size: usize) -> (Vec<m128i>, [m128i; 4]) {
    debug_assert!(output_size % 4 == 0);

    // Preallocate the result vector with proper capacity
    let mut result: Vec<m128i> = vec![m128i::zero(); output_size];
    
    // Fill the vector and get the new seed
    let new_seed = fill_aes_1rx4_m128i(input, &mut result);
    
    (result, new_seed)
}

#[allow(overflowing_literals)]
pub fn gen_program_aes_4rx4(input: &[m128i; 4], output_size: usize) -> Vec<m128i> {
    debug_assert!(output_size % 4 == 0);
    
    // Preallocate with exact capacity to avoid reallocations
    let mut result = Vec::with_capacity(output_size);
    
    // AES round keys (constants)
    let key0 = m128i::from_i32(0x99e5d23f, 0x2f546d2b, 0xd1833ddb, 0x6421aadd);
    let key1 = m128i::from_i32(0xa5dfcde5, 0x06f79d53, 0xb6913f55, 0xb20e3450);
    let key2 = m128i::from_i32(0x171c02bf, 0x0aa4679f, 0x515e7baf, 0x5c3ed904);
    let key3 = m128i::from_i32(0xd8ded291, 0xcd673785, 0xe78f5d08, 0x85623763);
    let key4 = m128i::from_i32(0x229effb4, 0x3d518b6d, 0xe3d6a7a6, 0xb5826f73);
    let key5 = m128i::from_i32(0xb272b7d2, 0xe9024d4e, 0x9c10b3d9, 0xc7566bf3);
    let key6 = m128i::from_i32(0xf63befa7, 0x2ba9660a, 0xf765a38b, 0xf273c9e7);
    let key7 = m128i::from_i32(0xc0b0762d, 0x0c06d1fd, 0x915839de, 0x7a7cd609);

    // Initialize states from input
    let mut state0 = input[0];
    let mut state1 = input[1];
    let mut state2 = input[2];
    let mut state3 = input[3];

    // Calculate the number of iterations needed
    let iterations = output_size / 4;
    
    // Reserve exact space in result vector
    result.reserve_exact(output_size);
    
    // Process each chunk
    for _ in 0..iterations {
        // First round of AES operations
        state0 = state0.aesdec(key0);
        state1 = state1.aesenc(key0);
        state2 = state2.aesdec(key4);
        state3 = state3.aesenc(key4);
        
        // Second round of AES operations
        state0 = state0.aesdec(key1);
        state1 = state1.aesenc(key1);
        state2 = state2.aesdec(key5);
        state3 = state3.aesenc(key5);

        // Third round of AES operations
        state0 = state0.aesdec(key2);
        state1 = state1.aesenc(key2);
        state2 = state2.aesdec(key6);
        state3 = state3.aesenc(key6);
        
        // Fourth round of AES operations
        state0 = state0.aesdec(key3);
        state1 = state1.aesenc(key3);
        state2 = state2.aesdec(key7);
        state3 = state3.aesenc(key7);

        // Store results
        result.push(state0);
        result.push(state1);
        result.push(state2);
        result.push(state3);
    }
    
    result
}
