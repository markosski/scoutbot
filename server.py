import socket
import serial
import time

def retry(max_retries=3, delay=1, exceptions=(Exception,)):
    def decorator(func):
        def wrapper(*args, **kwargs):
            retries = 0
            while retries < max_retries:
                try:
                    return func(*args, **kwargs)
                except exceptions as e:
                    retries += 1
                    print(f"Attempt {retries} failed. Retrying in {delay} seconds...")
                    time.sleep(delay)
            raise Exception("Maximum retries exceeded")
        return wrapper
    return decorator


@retry(max_retries=100, delay=3)
def serial_conn():
    ser = serial.Serial(
        #port='/dev/tty.usbmodem14201',  # Replace with your port name
        port='/dev/ttyACM0',  # Replace with your port name
        baudrate=9600        # Match the device's baud rate
    )
    return ser

def read():
    print("Start UDP listener")
    while True:
        # Receive data from the client
        data, client_address = udp_socket.recvfrom(1)  # Buffer size is 1024 bytes
        print(f"Received message from {client_address}: {data.decode()}")
        ser.write(data)

if __name__ == '__main__':
    local_address = '0.0.0.0'  # Replace with the target IP address
    local_port = 12345
    udp_socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    udp_socket.bind((local_address, local_port))

    ser = serial_conn()
    read()

