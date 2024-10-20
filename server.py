import socket
import serial

local_address = '0.0.0.0'  # Replace with the target IP address
local_port = 12345
udp_socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
udp_socket.bind((local_address, local_port))

ser = serial.Serial(
    port='/dev/tty.usbmodem14201',  # Replace with your port name
    baudrate=9600        # Match the device's baud rate
)

def read():
    print("Start UDP listener")
    while True:
        # Receive data from the client
        data, client_address = udp_socket.recvfrom(1)  # Buffer size is 1024 bytes
        print(f"Received message from {client_address}: {data.decode()}")
        ser.write(data)

if __name__ == '__main__':
    read()
