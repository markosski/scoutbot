import serial

# Configure the serial port
ser = serial.Serial(
    port='/dev/tty.usbmodem14201',  # Replace with your port name
    baudrate=9600        # Match the device's baud rate
)

while True:
    user_input = input("Enter movement (s, f, b, l, r, x, y): ")
    ser.write(bytes(user_input, 'utf-8'))
