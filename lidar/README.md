# RPLIDAR C1 Viewer

A native Rust utility for displaying live 2D scans from a Slamtec RPLIDAR C1. It speaks the RPLIDAR standard-scan serial protocol directly and renders scans with `egui`.

## Run it

Install Rust, then from this directory run:

```bash
cargo run --release -- --simulate
```

The simulator draws a room and two obstacles, so you can verify the UI before connecting hardware.

For a real C1, connect its USB adapter and run:

```bash
cargo run --release -- --port /dev/ttyUSB0
```

The default C1 baud rate is **460800**. The port and baud rate can also be changed in the toolbar. On Linux, if opening the port returns `Permission denied`, add your user to the serial-port group and log in again:

```bash
sudo usermod -aG dialout "$USER"
```

Some distributions use `uucp` instead of `dialout`.

## Controls

- **Connect / Disconnect** starts or stops the selected serial device.
- **Simulate** switches to the built-in test scene.
- **Display range** controls the outer radius of the plot.
- **Front 180° only** switches between the full scan and a forward-facing semicircular view.
- **Connect nearby points** outlines continuous surfaces.
- **Export scan as CSV** writes the currently displayed revolution to the working directory.

The view uses the lidar as its origin, with 0° pointing up. Distance values are millimeters.

## Build a standalone binary

```bash
cargo build --release
```

The binary is written to `target/release/rplidar-viewer`.

## Troubleshooting

- Stop other programs that may already have the serial port open.
- Confirm the USB device with `ls /dev/ttyUSB* /dev/ttyACM*`.
- Leave the baud at 460800 for an RPLIDAR C1 unless its interface configuration was changed.
- This first version uses the protocol's standard scan mode. That is the most portable mode and is sufficient for visualization; express/dense scan modes can be added later for higher sample throughput.
