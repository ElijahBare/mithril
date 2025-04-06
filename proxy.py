import socket
import threading
import json
import time
import logging

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="[%(asctime)s] %(levelname)s: %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S"
)

# Pool configuration
REAL_POOL_HOST = 'gulf.moneroocean.stream'  # Without stratum+tcp:// prefix
REAL_POOL_PORT = 10128
PROXY_PORT = 3334

connections = []
shares_submitted = 0
start_time = time.time()

lock = threading.Lock()

def handle_client(client_socket, address):
    global shares_submitted

    # Connect to real pool
    pool_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        logging.info(f"Connecting to pool {REAL_POOL_HOST}:{REAL_POOL_PORT}")
        pool_socket.connect((REAL_POOL_HOST, REAL_POOL_PORT))
        logging.info(f"Connected to pool {REAL_POOL_HOST}:{REAL_POOL_PORT}")
    except Exception as e:
        logging.error(f"Failed to connect to pool: {e}")
        client_socket.close()
        return

    def forward(src, dst, is_from_client):
        global shares_submitted
        src_name = "Client" if is_from_client else "Pool"
        dst_name = "Pool" if is_from_client else "Client"
        
        while True:
            try:
                data = src.recv(4096)
                if not data:
                    logging.info(f"{src_name} connection closed")
                    break

                if is_from_client:
                    try:
                        messages = data.decode().split('\n')
                        for msg in messages:
                            if msg.strip():
                                try:
                                    payload = json.loads(msg)
                                    method = payload.get("method", "unknown")
                                    logging.info(f"Client -> Pool: {method}")
                                    
                                    if method == "submit":
                                        with lock:
                                            shares_submitted += 1
                                            logging.info(f"Share submitted! Total: {shares_submitted}")
                                            logging.info(f"Share details: {payload}")
                                            
                                    # Log all messages for debugging
                                    logging.info(f"Client message: {method} - {payload}")
                                except json.JSONDecodeError:
                                    pass
                    except Exception as e:
                        logging.error(f"Error parsing client message: {e}")
                else:
                    try:
                        # Try to decode and log pool responses
                        messages = data.decode().split('\n')
                        for msg in messages:
                            if msg.strip():
                                try:
                                    payload = json.loads(msg)
                                    if "result" in payload:
                                        result = payload["result"]
                                        if isinstance(result, dict) and "job" in result:
                                            logging.info("Received new job from pool")
                                except json.JSONDecodeError:
                                    pass
                    except Exception as e:
                        logging.error(f"Error parsing pool message: {e}")

                # Forward the data
                dst.sendall(data)
                logging.debug(f"Forwarded data from {src_name} to {dst_name}")
                
            except Exception as e:
                logging.error(f"Connection error in {src_name}->{dst_name} forwarding: {e}")
                break

        try:
            src.close()
            logging.debug(f"Closed {src_name} socket")
        except:
            pass
            
        try:
            dst.close()
            logging.debug(f"Closed {dst_name} socket")
        except:
            pass

    # Create and start forwarding threads
    t1 = threading.Thread(target=forward, args=(client_socket, pool_socket, True))
    t2 = threading.Thread(target=forward, args=(pool_socket, client_socket, False))
    t1.daemon = True
    t2.daemon = True
    t1.start()
    t2.start()
    
    with lock:
        connections.append((client_socket, pool_socket))
        logging.info(f"Added new connection. Total active connections: {len(connections)}")

def hashrate_monitor():
    while True:
        time.sleep(10)
        elapsed = time.time() - start_time
        with lock:
            current_connections = len(connections)
            current_shares = shares_submitted
            
        if elapsed > 0:
            hrate = current_shares / elapsed * 60  # shares per minute
            logging.info(f"Estimated Hashrate: {hrate:.2f} shares/min | Active connections: {current_connections}")

def main():
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    
    try:
        # Allow reuse of address to avoid "address already in use" errors
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind(('0.0.0.0', PROXY_PORT))
        server.listen(100)
        logging.info(f"Proxy server started on 0.0.0.0:{PROXY_PORT}")
        logging.info(f"Forwarding to {REAL_POOL_HOST}:{REAL_POOL_PORT}")
        
        # Start hashrate monitor thread
        monitor_thread = threading.Thread(target=hashrate_monitor, daemon=True)
        monitor_thread.start()
        
        # Accept incoming connections
        while True:
            client_sock, addr = server.accept()
            logging.info(f"New connection from {addr[0]}:{addr[1]}")
            client_thread = threading.Thread(target=handle_client, args=(client_sock, addr))
            client_thread.daemon = True
            client_thread.start()
            
    except KeyboardInterrupt:
        logging.info("Shutting down proxy server...")
    except Exception as e:
        logging.error(f"Server error: {e}")
    finally:
        # Close all connections
        with lock:
            for client_sock, pool_sock in connections:
                try:
                    client_sock.close()
                    pool_sock.close()
                except:
                    pass
        try:
            server.close()
        except:
            pass
        logging.info("Proxy server stopped")

if __name__ == "__main__":
    main()
