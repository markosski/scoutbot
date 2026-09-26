use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::{SourceEvent, protocol::ScanPoint};

pub fn run(stop: Arc<AtomicBool>, tx: Sender<SourceEvent>) {
    if tx
        .send(SourceEvent::Connected("Simulated room".to_owned()))
        .is_err()
    {
        return;
    }
    let started = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let phase = started.elapsed().as_secs_f32();
        let scan = simulated_room(phase);
        if tx.send(SourceEvent::Scan(scan)).is_err() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn simulated_room(phase: f32) -> Vec<ScanPoint> {
    (0..720)
        .map(|index| {
            let angle = index as f32 * TAU / 720.0;
            let direction = (angle.sin(), -angle.cos());
            let wall_distance = ray_box_distance(direction, 2_600.0, 1_900.0);
            let obstacle = ray_circle_distance(direction, (700.0, -500.0), 320.0)
                .into_iter()
                .chain(ray_circle_distance(
                    direction,
                    (-900.0 + phase.sin() * 180.0, 450.0),
                    240.0,
                ))
                .fold(wall_distance, f32::min);
            let noise = ((index as f32 * 12.9898 + phase * 7.0).sin() * 8.0).round();
            ScanPoint {
                angle_deg: index as f32 * 0.5,
                distance_mm: obstacle + noise,
                quality: 48 + (index % 15) as u8,
                starts_new_scan: index == 0,
            }
        })
        .collect()
}

fn ray_box_distance(direction: (f32, f32), half_width: f32, half_height: f32) -> f32 {
    let x = if direction.0.abs() > 0.0001 {
        half_width / direction.0.abs()
    } else {
        f32::INFINITY
    };
    let y = if direction.1.abs() > 0.0001 {
        half_height / direction.1.abs()
    } else {
        f32::INFINITY
    };
    x.min(y)
}

fn ray_circle_distance(direction: (f32, f32), center: (f32, f32), radius: f32) -> Option<f32> {
    let projection = direction.0 * center.0 + direction.1 * center.1;
    let center_squared = center.0 * center.0 + center.1 * center.1;
    let discriminant = projection * projection - (center_squared - radius * radius);
    if discriminant < 0.0 {
        return None;
    }
    let distance = projection - discriminant.sqrt();
    (distance > 0.0).then_some(distance)
}
