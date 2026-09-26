# Scoutbot indoor autonomy and equipment strategy

## Recommendation

Build Scoutbot as a conventional, map-based indoor mobile robot and put language above the navigation stack:

```text
typed text or microphone
        |
speech-to-text (if voice) -> constrained intent parser
        |                    (navigate, describe, dock, stop)
        v
named room/dock goals -> Nav2 task executive
        |
map + localization -> global path -> local controller
        |                              |
        +---- lidar/depth costmap -----+
                                       v
                       collision monitor + hardware stop chain
                                       |
                              velocity-controlled base
```

Do **not** ask a vision-language model to steer the motors. Use language or a VLM to select a known destination or describe a camera image; use deterministic localization, planning, collision checking, command timeouts, and motor control to move the robot.

For this particular project, the long-term recommended navigation sensor suite is:

1. wheel encoders on every driven wheel;
2. a rigidly mounted IMU;
3. a 360-degree 2D lidar;
4. bumper, cliff, and short-range sensing plus a physical emergency stop;
5. a 64-bit onboard computer and correctly sized regulated power rails;
6. for charging, a battery-compatible dock with contacts, battery-current telemetry, and a visual dock marker.

The existing RGB camera remains useful for scene descriptions, AprilTag detection, and semantic confirmation. It should not be the only collision or localization sensor.

## First iteration decision: lidar-only odometry

The first working iteration deliberately reduces hardware and integration scope. It will use the existing RGB camera and a 360-degree 2D lidar, with **no wheel encoders and no IMU**. Final alignment with the charger remains manual through the existing camera feed and controls. A dedicated physical emergency-stop button is deferred during bench development; it is not replaced by software, and floor tests must remain directly supervised with the existing master power disconnect immediately reachable.

### Goals included in this iteration

- Manually survey and save one floor as a 2D occupancy map.
- Navigate autonomously to manually labeled `kitchen`, `hallway`, and `charger_staging` poses.
- Stop at the charger staging pose and hand control back to the operator for final docking.
- Answer “tell me what you see” from a fresh camera frame, preferably while stationary.
- Translate language only into allowlisted high-level actions and known destinations.

Unknown-object exploration, automatic charging alignment, autonomous reverse or strafing, and unrestricted operation around people are outside this iteration.

### Minimum first-iteration equipment

1. The existing camera, Raspberry Pi, Arduino/motor controller, battery, and manual controls.
2. One 360-degree indoor 2D lidar. A stable 10 Hz unit is preferred because lidar is the only motion sensor; a slower Neato or A1-class scanner requires proportionally slower motion and must pass the scan-stability test below.
3. A front bumper input to the motor controller. Add cliff sensors before operating anywhere a stair or drop-off is reachable.
4. A reachable battery/master power disconnect during supervised testing. Install a dedicated latching E-stop before extended or unsupervised autonomous operation.

### Encoder- and IMU-free software path

```text
lidar driver -> /scan -> lidar scan matcher -> /odom + odom->base_link
                                      |
                                      +-> SLAM Toolbox while mapping
                                      +-> AMCL + Nav2 with the saved map

camera -> stationary scene description
language -> validated named goal -> Nav2
Nav2 command -> TTL/watchdog gateway -> Arduino -> motors
```

Use lidar scan matching, such as RF2O, to estimate movement between scans. Nav2 does not require wheel encoders specifically, but it does require continuous odometry pose/velocity and the `odom -> base_link` transform; lidar is therefore the sole odometry source in this iteration ([Nav2 odometry guide](https://docs.nav2.org/rolling/configuration_and_development/first_time_robot_setup_guide/odom/setup_odom/), [Nav2 state estimation](https://docs.nav2.org/rolling/getting_started/navigation_concepts/state_estimation/)). The IMU upgrade remains straightforward later: fuse its angular velocity with lidar odometry using `robot_localization`.

Start with forward movement and in-place turns only. Use approximately `0.05-0.10 m/s` forward speed and `0.2 rad/s` turning speed, then raise limits only from measured results. Disable autonomous strafing and general reversing. A stale lidar scan, a stopped scan matcher, an implausible pose jump, lost global localization, an expired command, or a bumper/cliff event must produce an explicit zero command.

### Safety work that cannot be deferred

Before any powered autonomous floor test, replace the current indefinite direction-byte behavior with commands that carry a sequence number and short TTL. The Arduino must independently command zero when the TTL expires, serial traffic stops, a message is invalid, or a bumper/cliff input activates. `/stop` must send an explicit zero/stop command rather than only cancel the UDP task. Verify this first with the wheels raised by killing Wi-Fi, Axum, the Python bridge, and the lidar process one at a time.

Deferring the dedicated E-stop applies only to supervised prototype work. During every floor test, an operator must stay beside the reachable master power disconnect. Install the latching E-stop before the robot operates without that immediate supervision.

### Lidar qualification and release gate

Before mapping the home, run the scanner stationary for at least 30 minutes and confirm stable scan rate, timestamps, RPM where available, and complete 360-degree geometry. Then manually drive a slow square route with four roughly 90-degree turns. Do not enable autonomous goals unless repeated trials show all of the following:

- walls remain single and straight rather than duplicated or bent;
- turns do not cause large odometry jumps;
- the estimated endpoint returns reasonably near the starting point;
- stopping the lidar stream causes an immediate commanded stop;
- the saved map supports repeatable relocalization from several starting poses.

If lidar-only odometry repeatedly loses tracking in corridors, open rooms, glass-heavy areas, or slow turns, add the IMU before expanding the operating area. If stopping accuracy, stall detection, or motion consistency remains inadequate after that, wheel encoders are the next upgrade.

## Why the current robot cannot yet do this safely

The repository currently implements open-loop directional teleoperation:

- The web UI exposes forward, backward, lateral, yaw, and stop controls and displays an MJPEG stream ([`index.html`](../axum_server/index.html#L50-L68)).
- Axum accepts any string in `/move/:direction` and repeats it over UDP every 50 ms until another request cancels the task ([`main.rs`](../axum_server/src/main.rs#L64-L111)).
- The Python bridge forwards only one byte to the Arduino at 9600 baud; it receives no encoder, battery, fault, or acknowledgement data ([`server.py`](../server.py#L21-L45)).
- `/stop` stops the Axum sender but does not explicitly transmit the `s` byte to the motor controller ([`main.rs`](../axum_server/src/main.rs#L114-L144)). Whether the chassis stops therefore depends on Arduino firmware that is not in this repository.
- Deployment currently targets 32-bit ARMv7 ([`deploy`](../axum_server/deploy#L8-L13)). ROS 2 Jazzy's packaged Ubuntu targets are 64-bit x86 and ARM; ARM32 is Tier 3/source-only in REP-2000 ([ROS 2 Jazzy installation](https://docs.ros.org/en/jazzy/Installation/Alternatives/Ubuntu-Install-Binary.html), [REP-2000](https://www.ros.org/reps/rep-2000.html)).

Hardware alone will not close these gaps. Before autonomous testing, replace indefinite direction bytes with bounded velocity commands, add feedback, and make stale input stop the motors at the microcontroller.

## Equipment to add

### Minimum viable indoor autonomy

| Equipment | Practical class/specification | Interface and role | Why it is required |
|---|---|---|---|
| **Wheel encoders** | Quadrature encoders on each driven wheel or motor/gearbox; enough resolution to measure low-speed motion without long zero-count intervals | Encoder timer inputs on the Arduino or a dedicated motor-control MCU; return wheel position/velocity over USB serial or CAN | Supplies measured motion and enables closed-loop wheel velocity. Nav2 documents wheel encoders plus an IMU as a usual odometry setup; it requires timestamped odometry and the `odom -> base_link` transform ([Nav2 odometry guide](https://docs.nav2.org/rolling/configuration_and_development/first_time_robot_setup_guide/odom/setup_odom/), [sensor-fusion guide](https://docs.nav2.org/rolling/configuration_and_development/first_time_robot_setup_guide/odom/setup_robot_localization/)). For a four-wheel omnidirectional base, instrument all four wheels and confirm the kinematic model; for differential drive, instrument left and right drive sides. |
| **IMU** | 6-axis gyro/accelerometer is sufficient; 9-axis is acceptable but indoor magnetometer readings should not be trusted until tested and calibrated | SPI or I2C to the MCU/SBC; publish timestamped `sensor_msgs/Imu` with covariance | Stabilizes heading/rate between global corrections. Nav2's standard state-estimation chain fuses encoders, IMU, and optionally vision into smooth local odometry ([Nav2 state estimation](https://docs.nav2.org/rolling/getting_started/navigation_concepts/state_estimation/)). |
| **360-degree 2D lidar** | Indoor planar scanner; roughly 8-12 m useful range and 5-10 Hz or better is ample for a slow home robot; select a unit with a maintained ROS 2 driver | USB preferred, or UART through a reliable adapter; publish `sensor_msgs/LaserScan` | Builds the first occupancy map, localizes against walls, and feeds obstacle costmaps. Nav2 explicitly uses `LaserScan` with SLAM Toolbox, AMCL, and 2D costmaps ([Nav2 sensor guide](https://docs.nav2.org/rolling/configuration_and_development/first_time_robot_setup_guide/sensors/setup_sensors/)). A representative low-cost unit, SLAMTEC RPLIDAR A1, is specified for 360 degrees, 0.15-12 m, 5.5 Hz nominal scanning, UART/USB, and indoor use away from direct sunlight ([official datasheet](https://wiki.slamtec.com/download/attachments/83066883/LD108_SLAMTEC_rplidar_datasheet_A1M8_v3.0_en.pdf?api=v2&modificationDate=1677786044000&version=1)). This is an example class, not a mandatory model. |
| **Independent near-field safety sensors** | Perimeter bumper switches; downward-facing cliff sensors at leading corners; short-range ToF/IR at least front and rear | Wire to the MCU so a hit, cliff, or stale reading can stop motion without Linux, Wi-Fi, or the AI process | A single lidar scan plane can miss table edges, very low objects, drop-offs, and some difficult surfaces. Reverse should remain disabled until rear clearance is sensed. ROS can consume 1-D range sensors in a costmap, but the final stop should also exist below ROS ([Nav2 range layer](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/core_servers/costmap_2d/costmap_plugins/range/)). |
| **Physical emergency stop and watchdog** | Latching, reachable E-stop that removes motor-enable/power; MCU command watchdog; motor-driver fault/current input | Hard-wired stop chain plus a heartbeat/TTL in the command protocol | Nav2 Collision Monitor can stop or slow from fresh sensor data, but its own documentation says CPU-level monitoring is not hard-real-time safety certification ([official Collision Monitor documentation](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/core_servers/collision_monitor/configuring_collision_monitor_node/)). |
| **64-bit onboard computer** | Recommended baseline: Raspberry Pi 5, 8 GB, active cooling, high-endurance storage; keep the Arduino for hard real-time motor I/O | USB for lidar/controller, CSI/USB for camera, Ethernet or Wi-Fi for optional remote inference | Moves the project onto a supported ARM64 ROS 2/Nav2 path. Pi 5 provides a 2.4 GHz quad-core 64-bit Cortex-A76, USB 3, dual camera/display interfaces, and PCIe; Raspberry Pi recommends a 5 V/5 A supply and active cooling for best performance ([official Pi 5 specifications](https://www.raspberrypi.com/products/raspberry-pi-5/)). The existing ARMv7 Pi could remain a bridge during migration, but it is a poor long-term Nav2 host. |
| **Power distribution and telemetry** | Fused battery branch; separate regulated rails sized for motors, computer, and sensors; common signal ground where required; pack voltage/current/temperature sensing; correct BMS | Battery monitor to MCU/SBC; publish ROS `BatteryState` | Compute brownouts and motor noise can create unsafe resets. Power budget must include motor stall current and SBC peak load, not just average consumption. Battery chemistry, cell count, BMS, charger, connectors, and wire/fuse ratings must be mutually compatible. |
| **Charging dock hardware** | Wide or spring-loaded contacts, mechanical alignment/funnel, charger matched exactly to the pack/BMS, and a high-contrast AprilTag fixed to the dock | Dock pose from existing camera; charge-current/voltage confirmation from battery monitor | Nav2's docking framework supports a dock database, detected dock poses, joint/motor state, and `BatteryState`; its tutorial uses AprilTags plus battery current to confirm charging ([Nav2 docking tutorial](https://docs.nav2.org/jazzy/tutorials/general_tutorials/using_docking/)). Do not choose a dock charger until the battery voltage, chemistry, maximum charge current, and BMS behavior are known. |

Mount the lidar above the chassis with an unobstructed horizontal view, but low enough to see furniture legs. Measure and publish rigid transforms from every sensor to `base_link`. Place cliff sensors so braking distance at the configured maximum speed ends before a stair edge.

### Equipment needed only for spoken language

Typed commands through a browser or app require no new audio hardware. For voice commands, add:

- a USB microphone or small microphone array, positioned away from motors and fans;
- optionally a speaker for acknowledgement and spoken scene descriptions;
- a push-to-talk button or wake-word path, with **stop** handled as a high-priority local intent.

Speech recognition and the language model must never be in the emergency-stop path. Confirm ambiguous destinations instead of guessing.

### Robust optional upgrades

| Upgrade | Add it when | Value and integration |
|---|---|---|
| **Stereo/RGB-D camera with synchronized IMU** | The 2D lidar misses low/overhanging obstacles, richer scene understanding is needed, or visual-inertial odometry is desired | Feed a point cloud to the local costmap and keep the existing RGB feed for descriptions. A representative OAK-D connects over USB 2/3, produces stereo depth, and includes a BNO086 9-axis IMU ([official OAK-D specifications](https://docs.luxonis.com/hardware/products/OAK-D)). ROS can also project a depth image to `LaserScan` for localization/navigation ([`depthimage_to_laserscan`](https://docs.ros.org/en/ros2_packages/humble/api/depthimage_to_laserscan/index.html)). It complements rather than replaces bumpers/cliff sensors. |
| **AprilTags at ambiguous junctions** | Corridors or rooms look alike, or reliable cold-start recovery is needed | Print inexpensive, known-size tags and survey their poses in the map. The existing camera supplies absolute corrections. A tag at the charging dock is recommended even if tags elsewhere are omitted. |
| **Rear/side depth or additional ToF** | Autonomous reverse, strafing, or operation near people is enabled | Provides coverage outside the front camera and the lidar plane. For an omnidirectional chassis, sensing and collision zones must cover every commanded direction. |
| **Jetson-class edge compute** | Fully local VLM, dense depth/segmentation, or several neural pipelines cannot meet measured deadlines on Pi 5 | Jetson Orin Nano Super provides 8 GB LPDDR5, up to 67 INT8 TOPS, and 7-25 W modes ([official NVIDIA specifications](https://www.nvidia.com/en-sg/autonomous-machines/embedded-systems/jetson-orin/nano-super-developer-kit/)). It requires a larger energy and thermal budget. A remote GPU is also viable for scene descriptions, but movement must stop safely on network loss. |
| **UWB anchors** | Lidar/vision localization remains ambiguous across repeated multi-room geometry and installing infrastructure is acceptable | Adds absolute `x,y` corrections. It does not replace encoders/IMU or directly provide robust heading with one robot tag. Treat it as a later localization aid, not an MVP purchase. |

Do not prioritize a second ordinary monocular camera, a large onboard VLM, or UWB ahead of encoders, lidar, and the hard-wired stop chain.

## How each requested command should work

| User command | Deterministic execution |
|---|---|
| **“Find the kitchen and stay there.”** | In the MVP, `kitchen` is a room polygon plus one or more safe goal poses labeled during mapping. The language layer resolves the name, Nav2 drives to the selected pose, and the task ends with zero velocity while localization/collision monitoring remains active. “Find” should not mean unguided visual exploration until a separately tested semantic-exploration behavior exists. |
| **“Go to the hallway.”** | Resolve `hallway` to a named pose/region and call `goToPose`. Nav2's Simple Commander exposes non-blocking `goToPose`, task feedback, cancellation, routes, and docking calls ([official Simple Commander API](https://docs.nav2.org/rolling/configuration_and_development/simple_commander_api/simple_commander_api/)). |
| **“Tell me what you see.”** | Stop or remain stationary, capture a fresh camera frame, and send it to a local or remote vision-language model. Return a description with an “uncertain” outcome when image quality is poor. If a panoramic answer is wanted, make “scan and describe” a separate, slow, bounded rotation behavior with collision checks. |
| **“Go to the charging station.”** | In the first iteration, navigate to a mapped staging pose, stop completely, notify the operator, and return control for manual final alignment and charging. Automatic visual alignment and charge confirmation remain a later upgrade. |

The language model should return a small validated object such as:

```json
{"intent":"navigate","target":"kitchen","on_arrival":"stay"}
```

Only allow a fixed intent set (`navigate`, `describe`, `dock`, `stop`, `status`) and destinations that exist in the semantic map. Reject invented room names, out-of-range values, and direct motor instructions.

## Recommended software stack

Use ROS 2 Jazzy on 64-bit Ubuntu with Nav2:

- `ros2_control` plus the appropriate differential/omnidirectional controller for measured wheel velocity, odometry, acceleration limits, and `cmd_vel`; the official differential-drive controller translates body velocity into wheel commands and publishes encoder odometry ([controller documentation](https://control.ros.org/jazzy/doc/ros2_controllers/diff_drive_controller/doc/userdoc.html)).
- `robot_localization` to fuse encoder odometry and IMU into smooth `odom -> base_link`.
- SLAM Toolbox while manually mapping the home with lidar, then AMCL or SLAM Toolbox localization against the saved map. SLAM Toolbox consumes `LaserScan` plus odometry and can save the occupancy map and pose graph ([official project](https://github.com/SteveMacenski/slam_toolbox)).
- Nav2 map/costmap, planner, controller, behavior tree, velocity smoother, Collision Monitor, and Docking Server.
- A small application node containing a `named_goals.yaml` file with room polygons, safe poses, aliases, and the dock staging pose.
- A separate language/vision service. It may run on the robot, a LAN GPU, or a cloud API, but all navigation commands must remain cancelable and the robot must stop on stale data or disconnection.

Maintain the standard transform chain `map -> odom -> base_link -> sensor frames`; Nav2 identifies it as the minimum navigation frame tree ([state-estimation documentation](https://docs.nav2.org/rolling/getting_started/navigation_concepts/state_estimation/)).

## Required controller changes before the first autonomous drive

1. Define the chassis kinematics and measure wheel radius, wheel separation/geometry, maximum speed, acceleration, braking distance, and motor deadband.
2. Replace the one-byte indefinite command with a framed message such as `{sequence, timestamp, ttl_ms, vx, vy, wz}` plus telemetry and acknowledgement. The MCU must command zero when the TTL expires, serial data is malformed, an encoder fault occurs, or a bumper/cliff input fires.
3. Make stop preemptive and explicit at every layer. `/stop` must transmit zero/stop, not merely stop the UDP loop.
4. Validate command ranges and authentication/network exposure. The current Axum route accepts an arbitrary direction string.
5. Add timestamps and log camera frames, scans, odometry, IMU, battery, planned path, commands, faults, and operator interventions.

## Staged implementation plan

1. **Safe base:** install encoders, IMU, bumper/cliff sensors, E-stop, power telemetry, and the MCU watchdog. Prove that unplugging Wi-Fi, killing Axum/Python, unplugging a sensor, or freezing commands always stops the base.
2. **Closed-loop motion:** expose measured odometry and accept bounded continuous velocity. Calibrate forward distance, rotation, braking distance, and footprint at very low speed.
3. **One-room navigation:** install lidar, make a map with SLAM Toolbox, localize, and tune Nav2 in an empty bounded room. Keep a person at the E-stop.
4. **Whole-floor semantic map:** map the usable floor, add keep-out zones for stairs and fragile areas, then label kitchen, hallway, and safe waiting poses. Test direct UI-selected goals before adding language.
5. **Language and descriptions:** translate text/voice into the fixed intent schema. Add `describe` using the existing camera. Run every new intent in shadow mode before it can actuate.
6. **Charging:** install the electrically matched dock, add its AprilTag and staging pose, publish battery status, and tune slow docking with repeated supervised trials.
7. **Robustness upgrades:** add RGB-D/rear coverage, tags at ambiguous locations, or stronger compute only in response to measured failure modes.

Suggested initial release gates are: no unintended motion after any single software/network failure tested; zero false-forward events toward a cliff or obstacle in the bounded test course; repeatable localization after restart; successful arrival at every named goal from multiple starts; and charging confirmed electrically rather than inferred from pose alone.

## Short purchase order

If buying in phases, the order should be:

1. E-stop, bumper switches, cliff sensors, MCU watchdog-capable control hardware, and proper fused/regulator power distribution.
2. Encoders for every driven wheel and one IMU.
3. One supported 360-degree indoor 2D lidar.
4. Raspberry Pi 5 (8 GB), active cooling, 64-bit storage/OS, and adequate 5 V power, unless equivalent ARM64/x86 compute is already available.
5. Battery monitor plus a battery/BMS-compatible charging dock, contacts, and AprilTag.
6. USB microphone only if spoken rather than typed commands are required.
7. Optional RGB-D/IMU camera and Jetson-class compute after profiling and real-house trials.
