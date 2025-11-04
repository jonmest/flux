import time
import socket
import threading

def make_request():
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.connect(('localhost', 3000))  # Direct to backend, not Flux
        sock.sendall(b"test\n")
        response = sock.recv(1024)
        sock.close()
        return 1
    except:
        return 0

def benchmark(duration_sec, num_threads):
    results = []
    stop_flag = threading.Event()
    
    def worker():
        count = 0
        while not stop_flag.is_set():
            count += make_request()
        results.append(count)
    
    threads = [threading.Thread(target=worker) for _ in range(num_threads)]
    
    start = time.time()
    for t in threads:
        t.start()
    
    time.sleep(duration_sec)
    stop_flag.set()
    
    for t in threads:
        t.join()
    
    total = sum(results)
    elapsed = time.time() - start
    
    print(f"Total requests: {total}")
    print(f"Duration: {elapsed:.2f}s")
    print(f"Requests/sec: {total/elapsed:.2f}")

if __name__ == '__main__':
    benchmark(duration_sec=30, num_threads=10)