mod protocol;
mod simulator;

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, Sender},
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Sense, Stroke, Vec2};
use protocol::{ScanDecoder, ScanPoint, begin_scan};

const DEFAULT_BAUD: u32 = 460_800;
const MIN_FRAME_POINTS: usize = 30;

#[derive(Default)]
struct LaunchOptions {
    port: Option<String>,
    baud: u32,
    simulate: bool,
}

impl LaunchOptions {
    fn parse() -> Self {
        let mut result = Self {
            baud: DEFAULT_BAUD,
            ..Self::default()
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--simulate" => result.simulate = true,
                "--port" => result.port = args.next(),
                "--baud" => {
                    if let Some(value) = args.next() {
                        result.baud = value.parse().unwrap_or(DEFAULT_BAUD);
                    }
                }
                "--help" | "-h" => {
                    println!(
                        "RPLIDAR C1 Viewer\n\n  --port <device>  Serial device (for example /dev/ttyUSB0)\n  --baud <rate>    Serial baud rate (default: {DEFAULT_BAUD})\n  --simulate       Start with a simulated room scan"
                    );
                    std::process::exit(0);
                }
                _ => eprintln!("Ignoring unknown option: {arg}"),
            }
        }
        result
    }
}

enum SourceEvent {
    Connected(String),
    Scan(Vec<ScanPoint>),
    Error(String),
    Stopped,
}

struct ActiveSource {
    stop: Arc<AtomicBool>,
}

impl ActiveSource {
    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for ActiveSource {
    fn drop(&mut self) {
        self.stop();
    }
}

struct ViewerApp {
    port: String,
    ports: Vec<String>,
    baud: u32,
    status: String,
    connected: bool,
    receiver: Receiver<SourceEvent>,
    sender: Sender<SourceEvent>,
    source: Option<ActiveSource>,
    scan: Vec<ScanPoint>,
    scan_count: u64,
    scan_rate_hz: f32,
    last_scan_at: Option<Instant>,
    max_range_mm: f32,
    point_radius: f32,
    draw_lines: bool,
    show_low_quality: bool,
    front_only: bool,
    last_export: Option<String>,
}

impl ViewerApp {
    fn new(options: LaunchOptions) -> Self {
        let (sender, receiver) = mpsc::channel();
        let ports = available_ports();
        let port = options
            .port
            .or_else(|| ports.first().cloned())
            .unwrap_or_else(|| "/dev/ttyUSB0".to_owned());
        let mut app = Self {
            port,
            ports,
            baud: options.baud,
            status: "Disconnected".to_owned(),
            connected: false,
            receiver,
            sender,
            source: None,
            scan: Vec::new(),
            scan_count: 0,
            scan_rate_hz: 0.0,
            last_scan_at: None,
            max_range_mm: 6_000.0,
            point_radius: 2.0,
            draw_lines: true,
            show_low_quality: true,
            front_only: false,
            last_export: None,
        };
        if options.simulate {
            app.start_simulator();
        }
        app
    }

    fn stop_source(&mut self) {
        if let Some(source) = self.source.take() {
            source.stop();
        }
        self.connected = false;
        self.status = "Disconnected".to_owned();
    }

    fn start_serial(&mut self) {
        self.stop_source();
        let port_name = self.port.clone();
        let baud = self.baud;
        let tx = self.sender.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        self.status = format!("Connecting to {port_name}…");

        thread::spawn(move || {
            let result = run_serial_source(&port_name, baud, &worker_stop, &tx);
            if let Err(error) = result {
                let _ = tx.send(SourceEvent::Error(error));
            } else {
                let _ = tx.send(SourceEvent::Stopped);
            }
        });
        self.source = Some(ActiveSource { stop });
    }

    fn start_simulator(&mut self) {
        self.stop_source();
        let tx = self.sender.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        self.status = "Starting simulator…".to_owned();
        thread::spawn(move || simulator::run(worker_stop, tx));
        self.source = Some(ActiveSource { stop });
    }

    fn receive_events(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                SourceEvent::Connected(name) => {
                    self.status = name;
                    self.connected = true;
                }
                SourceEvent::Scan(scan) => {
                    let now = Instant::now();
                    if let Some(last) = self.last_scan_at {
                        let instant_rate = 1.0 / now.duration_since(last).as_secs_f32().max(0.001);
                        self.scan_rate_hz = if self.scan_rate_hz == 0.0 {
                            instant_rate
                        } else {
                            self.scan_rate_hz * 0.85 + instant_rate * 0.15
                        };
                    }
                    self.last_scan_at = Some(now);
                    self.scan_count += 1;
                    self.scan = scan;
                }
                SourceEvent::Error(error) => {
                    self.status = format!("Error: {error}");
                    self.connected = false;
                    self.source = None;
                }
                SourceEvent::Stopped => {
                    if self.source.is_some() {
                        self.status = "Source stopped".to_owned();
                        self.connected = false;
                        self.source = None;
                    }
                }
            }
        }
    }

    fn export_scan(&mut self) {
        if self.scan.is_empty() {
            self.last_export = Some("Nothing to export yet".to_owned());
            return;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!("rplidar-scan-{timestamp}.csv");
        let result = (|| -> std::io::Result<()> {
            let mut file = File::create(&filename)?;
            writeln!(file, "angle_deg,distance_mm,quality")?;
            for point in &self.scan {
                writeln!(
                    file,
                    "{:.3},{:.2},{}",
                    point.angle_deg, point.distance_mm, point.quality
                )?;
            }
            Ok(())
        })();
        self.last_export = Some(match result {
            Ok(()) => format!("Saved {filename}"),
            Err(error) => format!("Export failed: {error}"),
        });
    }

    fn draw_scan(&self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
        let rect = response.rect;
        let (center, radius) = if self.front_only {
            (
                Pos2::new(rect.center().x, rect.bottom() - 24.0),
                (rect.width() * 0.5 - 36.0)
                    .min(rect.height() - 60.0)
                    .max(20.0),
            )
        } else {
            (
                rect.center(),
                (rect.width().min(rect.height()) * 0.5 - 36.0).max(20.0),
            )
        };
        let scale = radius / self.max_range_mm;
        let grid = Color32::from_gray(50);
        let text = Color32::from_gray(125);

        painter.rect_filled(rect, 0.0, Color32::from_rgb(12, 16, 22));
        for fraction in [0.25_f32, 0.5, 0.75, 1.0] {
            if self.front_only {
                let arc = (0..=64)
                    .map(|step| {
                        let angle = (-90.0_f32 + 180.0 * step as f32 / 64.0).to_radians();
                        center
                            + Vec2::new(
                                angle.sin() * radius * fraction,
                                -angle.cos() * radius * fraction,
                            )
                    })
                    .collect();
                painter.add(egui::Shape::line(arc, Stroke::new(1.0_f32, grid)));
            } else {
                painter.circle_stroke(center, radius * fraction, Stroke::new(1.0_f32, grid));
            }
            painter.text(
                center + Vec2::new(4.0, -radius * fraction + 3.0),
                Align2::LEFT_TOP,
                format!("{:.1} m", self.max_range_mm * fraction / 1000.0),
                FontId::monospace(11.0),
                text,
            );
        }
        painter.line_segment(
            [
                Pos2::new(center.x - radius, center.y),
                Pos2::new(center.x + radius, center.y),
            ],
            Stroke::new(1.0_f32, grid),
        );
        painter.line_segment(
            [
                Pos2::new(center.x, center.y - radius),
                Pos2::new(
                    center.x,
                    if self.front_only {
                        center.y
                    } else {
                        center.y + radius
                    },
                ),
            ],
            Stroke::new(1.0_f32, grid),
        );
        painter.text(
            center + Vec2::new(0.0, -radius - 5.0),
            Align2::CENTER_BOTTOM,
            "0°",
            FontId::monospace(11.0),
            text,
        );

        let visible: Vec<(Pos2, &ScanPoint)> = self
            .scan
            .iter()
            .filter(|point| {
                point.distance_mm > 0.0
                    && point.distance_mm <= self.max_range_mm
                    && (self.show_low_quality || point.quality > 0)
                    && (!self.front_only || is_front_angle(point.angle_deg))
            })
            .map(|point| {
                let angle = point.angle_deg.to_radians();
                let distance = point.distance_mm * scale;
                (
                    center + Vec2::new(angle.sin() * distance, -angle.cos() * distance),
                    point,
                )
            })
            .collect();

        if self.draw_lines {
            for pair in visible.windows(2) {
                let (a_pos, a) = pair[0];
                let (b_pos, b) = pair[1];
                let angle_gap = (b.angle_deg - a.angle_deg).abs();
                let range_gap = (b.distance_mm - a.distance_mm).abs();
                if angle_gap < 2.5 && range_gap < 250.0 {
                    painter.line_segment(
                        [a_pos, b_pos],
                        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(40, 190, 210, 95)),
                    );
                }
            }
        }
        for (position, point) in visible {
            let quality = point.quality as f32 / 63.0;
            let color = quality_color(quality);
            painter.circle_filled(position, self.point_radius, color);
        }

        painter.circle_filled(center, 5.0, Color32::WHITE);
        painter.line_segment(
            [center, center + Vec2::new(0.0, -15.0)],
            Stroke::new(2.0_f32, Color32::WHITE),
        );
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_events();

        egui::TopBottomPanel::top("connection_bar").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.heading("RPLIDAR C1");
                ui.separator();
                egui::ComboBox::from_id_salt("serial_port")
                    .selected_text(&self.port)
                    .width(155.0)
                    .show_ui(ui, |ui| {
                        for port in &self.ports {
                            ui.selectable_value(&mut self.port, port.clone(), port);
                        }
                    });
                if ui
                    .button("↻")
                    .on_hover_text("Refresh serial ports")
                    .clicked()
                {
                    self.ports = available_ports();
                    if !self.ports.is_empty() && !self.ports.contains(&self.port) {
                        self.port = self.ports[0].clone();
                    }
                }
                ui.label("Baud");
                ui.add(
                    egui::DragValue::new(&mut self.baud)
                        .speed(100)
                        .range(9_600..=2_000_000),
                );
                if self.connected || self.source.is_some() {
                    if ui.button("Disconnect").clicked() {
                        self.stop_source();
                    }
                } else if ui.button("Connect").clicked() {
                    self.start_serial();
                }
                if ui.button("Simulate").clicked() {
                    self.start_simulator();
                }
                ui.separator();
                let status_color = if self.connected {
                    Color32::from_rgb(80, 220, 140)
                } else if self.status.starts_with("Error") {
                    Color32::from_rgb(245, 100, 100)
                } else {
                    Color32::from_gray(170)
                };
                ui.colored_label(status_color, &self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(egui::RichText::new("×").size(18.0))
                        .on_hover_text("Close window")
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
            ui.add_space(5.0);
        });

        egui::SidePanel::right("settings")
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Scan");
                ui.add_space(8.0);
                egui::Grid::new("stats").num_columns(2).show(ui, |ui| {
                    ui.label("Points");
                    ui.monospace(self.scan.len().to_string());
                    ui.end_row();
                    ui.label("Rate");
                    ui.monospace(format!("{:.1} Hz", self.scan_rate_hz));
                    ui.end_row();
                    ui.label("Frames");
                    ui.monospace(self.scan_count.to_string());
                    ui.end_row();
                });
                ui.separator();
                ui.label("Display range");
                ui.add(
                    egui::Slider::new(&mut self.max_range_mm, 1_000.0..=12_000.0)
                        .suffix(" mm")
                        .logarithmic(true),
                );
                ui.label("Point size");
                ui.add(egui::Slider::new(&mut self.point_radius, 1.0..=5.0));
                ui.checkbox(&mut self.draw_lines, "Connect nearby points");
                ui.checkbox(&mut self.show_low_quality, "Show zero-quality points");
                ui.checkbox(&mut self.front_only, "Front 180° only")
                    .on_hover_text("Show measurements from 90° left through 90° right");
                ui.separator();
                if ui
                    .add_enabled(
                        !self.scan.is_empty(),
                        egui::Button::new("Export scan as CSV"),
                    )
                    .clicked()
                {
                    self.export_scan();
                }
                if let Some(message) = &self.last_export {
                    ui.small(message);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.small("0° is forward • distances in millimeters");
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| self.draw_scan(ui));

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn available_ports() -> Vec<String> {
    let mut ports: Vec<_> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| port.port_name)
        .collect();
    ports.sort();
    ports
}

fn run_serial_source(
    port_name: &str,
    baud: u32,
    stop: &AtomicBool,
    tx: &Sender<SourceEvent>,
) -> Result<(), String> {
    let mut port = serialport::new(port_name, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|error| format!("cannot open {port_name}: {error}"))?;

    begin_scan(&mut *port).map_err(|error| format!("scan startup failed: {error}"))?;
    tx.send(SourceEvent::Connected(format!("Scanning {port_name}")))
        .map_err(|_| "UI closed".to_owned())?;

    let mut decoder = ScanDecoder::default();
    let mut read_buffer = [0_u8; 1024];
    let mut frame = Vec::with_capacity(1_000);

    while !stop.load(Ordering::Relaxed) {
        match port.read(&mut read_buffer) {
            Ok(bytes_read) => {
                for &byte in &read_buffer[..bytes_read] {
                    if let Some(point) = decoder.push(byte) {
                        if point.starts_new_scan && frame.len() >= MIN_FRAME_POINTS {
                            let completed =
                                std::mem::replace(&mut frame, Vec::with_capacity(1_000));
                            if tx.send(SourceEvent::Scan(completed)).is_err() {
                                return Ok(());
                            }
                        }
                        frame.push(point);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("serial read failed: {error}")),
        }
    }
    protocol::stop_scan(&mut *port);
    Ok(())
}

fn quality_color(quality: f32) -> Color32 {
    let quality = quality.clamp(0.0, 1.0);
    Color32::from_rgb(
        (50.0 + quality * 170.0) as u8,
        (150.0 + quality * 90.0) as u8,
        (230.0 - quality * 90.0) as u8,
    )
}

fn is_front_angle(angle_deg: f32) -> bool {
    let angle = angle_deg.rem_euclid(360.0);
    angle <= 90.0 || angle >= 270.0
}

fn main() -> eframe::Result {
    let options = LaunchOptions::parse();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RPLIDAR C1 Viewer")
            .with_app_id("rplidar-viewer")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([820.0, 560.0])
            .with_decorations(true),
        ..Default::default()
    };
    eframe::run_native(
        "RPLIDAR C1 Viewer",
        native_options,
        Box::new(move |_creation_context| Ok(Box::new(ViewerApp::new(options)))),
    )
}

#[cfg(test)]
mod tests {
    use super::is_front_angle;

    #[test]
    fn front_view_includes_exactly_the_forward_half() {
        for angle in [-90.0, 0.0, 45.0, 90.0, 270.0, 360.0] {
            assert!(is_front_angle(angle), "expected {angle}° in front view");
        }
        for angle in [90.1, 180.0, 269.9] {
            assert!(!is_front_angle(angle), "expected {angle}° behind lidar");
        }
    }
}
