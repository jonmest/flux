import socket
import threading

def handle_client(client, addr):
    print(f"Connection from {addr}")
    try:
        while True:
            data = client.recv(1024)
            if not data:
                break
            client.send(b"Backend says: " + data)
    finally:
        print(f"Connection from {addr} closed")
        client.close()

def main():
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(('127.0.0.1', 3000))
    server.listen(5)
    print("Backend server listening on port 3000")
    
    while True:
        client, addr = server.accept()

        thread = threading.Thread(target=handle_client, args=(client, addr))
        thread.start()

if __name__ == '__main__':
    main()
