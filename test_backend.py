import socket
import threading
import sys

def handle_client(client, addr, port):
    print(f"[Port {port}] Connection from {addr}")
    try:
        while True:
            data = client.recv(1024)
            if not data:
                break
            response = f"Backend {port} says: {data.decode()}".encode()
            client.send(response)
    finally:
        print(f"[Port {port}] Connection from {addr} closed")
        client.close()

def main(port):
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(('127.0.0.1', port))
    server.listen(5)
    print(f"Backend server listening on port {port}")
    
    while True:
        client, addr = server.accept()
        thread = threading.Thread(target=handle_client, args=(client, addr, port))
        thread.start()

if __name__ == '__main__':
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 3000
    main(port)
