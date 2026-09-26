# Vision-based navigation research

## Recommendation in one sentence

Start with a **small, goal-conditioned image policy** (MobileNetV3-Small, five action logits plus a collision-risk head) running locally on the Raspberry Pi, but let a deterministic supervisor—not the model—own stopping, command timeouts, route following, and collision vetoes. Add floor-plan localization first with wheel odometry/IMU plus AprilTags at known map coordinates; evaluate visual-inertial SLAM or learned floor-plan matching only after the driving loop is reliable.

This is deliberately a hybrid design. A frame-only classifier can react to a corridor, but it cannot know which way to turn at an identical-looking intersection without a goal or route input. Likewise, a floor plan describes walls, not a chair, pet, person, or open stairwell that appeared today.

## What the repository already provides

- The browser UI displays an MJPEG stream and exposes `f`, `b`, `l`, `r`, `x`, `y`, and stop controls ([`index.html`](../axum_server/index.html#L50-L68)). Confirm whether `l/r` means lateral translation and `x/y` means yaw before defining the model's action vocabulary.
- Axum starts a background task for `/move/:direction` and repeats the chosen byte over UDP every 50 ms until another command or `/stop` arrives ([`main.rs`](../axum_server/src/main.rs#L64-L111)). The path currently accepts any string and has no lease, sequence number, or stale-command timeout.
- Python forwards one-byte UDP packets to the Arduino at 9600 baud ([`server.py`](../server.py#L21-L45)).
- Deployment targets `armv7-unknown-linux-gnueabihf`, so the current Pi is using a 32-bit ARM target ([`deploy`](../axum_server/deploy#L8-L13)). This favors a compact CNN and LiteRT/NCNN over a large vision-language model.

## Proposed architecture

```text
camera (latest frame only)       floor plan -> occupancy grid -> A* route
           |                                      |
           v                                      v
  lightweight vision encoder <--- goal bearing / path error / pose uncertainty
           |       action probabilities + collision probability
           v
 deterministic supervisor <--- depth/range/bumper + localization health + watchdog
           |
           v
 Axum leased command API -> UDP -> serial -> motor controller watchdog
```

### 1. Local movement policy

Use a 160-224 px RGB input and a **MobileNetV3-Small** encoder, pretrained for images and fine-tuned on this robot. MobileNetV3-Small was explicitly designed for low-resource mobile CPUs ([paper](https://arxiv.org/abs/1905.02244)). Concatenate the image embedding with compact navigation state:

- sine/cosine of the bearing to the next waypoint;
- signed cross-track error and distance to waypoint;
- current/previous action and estimated speed;
- pose uncertainty and optionally two preceding frames.

Outputs should be `forward`, `left`, `right`, `backward`, and `stop` logits plus a separate collision-risk scalar. This is a classifier at the final layer, but a **goal-conditioned policy** in behavior. A DroNet-style second head is a useful precedent: DroNet uses one compact residual network to predict steering and collision probability from a forward camera and was designed for real-time CPU use ([project and paper](https://rpg.ifi.uzh.ch/dronet.html)). Its urban weights are not suitable for this house; borrow the architecture idea, not the checkpoint.

Prefer MobileNetV3 over a general object detector for the first prototype. COCO detection answers “what objects are present,” not “which local motion follows this route safely.” Detection or segmentation can become an auxiliary head later if logs show a concrete need. End-to-end camera-to-steering has precedent in NVIDIA's 30 FPS PilotNet experiments ([paper](https://arxiv.org/abs/1604.07316)), but the map, safety veto, and stop logic should remain explicit here.

Do not put a general-purpose VLM in the control loop. It can help translate a human instruction such as “go to the kitchen” into a named floor-plan goal or label difficult training frames offline, but token generation, prompt sensitivity, and resource use are poor fits for a deterministic 10-20 Hz motor loop.

## General-purpose server-side models

Yes—there are now pretrained, cross-robot navigation policies that are a much closer fit than training a five-way classifier from scratch. They still need a goal (or an explicit exploration mode), a robot-specific output adapter, and an independent safety controller. A current camera frame alone cannot determine whether the intended route turns left or right at an otherwise identical junction.

| Model family | What it accepts and produces | Readiness for Scoutbot |
|---|---|---|
| **NoMaD / ViNT / GNM** | Recent forward-camera frames plus a visual goal from a recorded topological map; NoMaD can also run goal-masked exploration. The released deployment code predicts a short local waypoint/trajectory and converts it to linear/angular velocity with a PD controller. | **Best first server-side experiment.** Official checkpoints and LoCoBot/TurtleBot deployment code are available, and the repository reports deployment on several other mobile robots. The reference stack is Ubuntu 20.04, ROS Noetic, PyTorch, and was tested on a Jetson Orin Nano; training assumes a CUDA GPU. [Official repository](https://github.com/robodhruv/visualnav-transformer), [NoMaD project](https://general-navigation-models.github.io/nomad/) |
| **OmniVLA / OmniVLA-edge** | Current RGB image plus any combination of natural-language instruction, egocentric goal image, and relative 2D goal pose. It predicts an eight-step 2D trajectory with heading, then the sample code derives linear/angular velocity. | **Most relevant richer option.** Full OmniVLA is a 7.5B/8B BF16 model and the sample loop runs at 3 Hz; the released 50M-parameter edge variant is the practical first trial. Its 2D-pose input can consume a waypoint derived from the house plan, but neither variant estimates the robot's floor-plan pose. [Project and paper](https://omnivla-nav.github.io/), [inference code and setup](https://github.com/NHirose/OmniVLA), [full checkpoint](https://huggingface.co/NHirose/omnivla-original), [edge checkpoint](https://huggingface.co/NHirose/omnivla-edge) |
| **NaVILA** | RGB video plus a language route instruction; emits mid-level language actions such as moving forward a distance or turning an angle, while a separate learned locomotion policy handles real-time execution and obstacle avoidance. | Interesting high-level planner, but its released system is designed around legged robots and the official repository still lists model weights/evaluation as unreleased. It is not a drop-in wheeled-base policy. [Project](https://navila-bot.github.io/), [official repository](https://github.com/AnjieCheng/NaVILA) |
| **OneVLA** | Multi-view RGB, language, and optional robot state; a 3B model uses one action head for discrete navigation and 7-DoF manipulation. | Promising but early: the released navigation path is centered on Habitat R2R/RxR evaluation rather than a generic wheeled-robot deployment. Treat it as a later benchmark, not the first controller. [Project](https://linglingxiansen.github.io/onevla.github.io/), [official repository](https://github.com/linglingxiansen/OneVLA) |

Do not confuse navigation policies with manipulation VLAs. **OpenVLA** accepts an image and instruction but its released action is a continuous 7-DoF arm/gripper vector trained from manipulation trajectories; its maintainers say new domains typically require target-domain fine-tuning and recommend roughly 5–10 Hz control data. It cannot drive this base merely by renaming its outputs `forward/left/right`. [Official OpenVLA repository](https://github.com/openvla/openvla) The same category warning applies to other arm-focused generalist policies. A general VLM such as **Qwen3-VL** can be prompted to return one JSON command from a frame and is useful as a slow semantic adviser, but its official model is an image/video-to-text model—not a calibrated mobile-robot policy. Use it to select a destination or subgoal, or to compare in shadow mode, rather than to own motor actuation. [Official Qwen3-VL repository](https://github.com/QwenLM/Qwen3-VL)

For this repository, begin with **NoMaD on the GPU server** and a recorded visual topological route. Stream only the newest camera context to the server, return the predicted local waypoint plus timestamp, and adapt that waypoint deterministically:

- `stop` on a stale response, missing context, reached goal, safety veto, or out-of-range output;
- yaw left/right when the waypoint bearing exceeds a tuned deadband, otherwise move forward;
- do not infer reverse from a front-camera navigation model; reserve it for a bounded recovery with rear sensing;
- refresh a 200–300 ms command lease rather than starting an indefinite Axum movement task.

The released NoMaD/ViNT controller assumes forward velocity plus yaw. In the current UI, `l/r` are labeled lateral motion while `x/y` are yaw, so the adapter should normally map steering to `x/y`, not to `l/r`, unless the chassis truly supports and was trained for holonomic strafing. OmniVLA is the second experiment if language goals or a relative floor-plan waypoint are important and the server can meet the measured end-to-end latency. In both cases, run inference in shadow mode first and benchmark **frame age + network time + inference + Axum actuation**, not inference time alone.

### 2. Runtime and hardware

- Capture a low-resolution camera stream locally and always overwrite the pending frame; never queue old frames. Raspberry Pi's Picamera2 has an official real-time LiteRT example, and Raspberry Pi's camera stack supports 224x224 MobileNet classification on a low-resolution stream ([Picamera2 example](https://github.com/raspberrypi/picamera2/blob/main/examples/tensorflow/real_time.py), [camera documentation](https://github.com/raspberrypi/documentation/blob/master/documentation/asciidoc/computers/camera/rpicam_apps_post_processing_tflite.adoc)). Avoid decoding the browser-facing MJPEG stream for control.
- Export an INT8 CNN and benchmark **on this exact ARMv7 Pi**. LiteRT is the lowest-risk starting runtime; NCNN is another ARM-oriented option. ONNX Runtime is attractive if the OS moves to 64-bit ARM; its official guidance recommends static quantization for CNNs and notes that quantization gains depend on the processor's instructions ([quantization guide](https://onnxruntime.ai/docs/performance/model-optimizations/quantization.html)).
- Treat 10 Hz, p95 inference under 50 ms, and camera-to-command under 100 ms as initial acceptance targets, not assumed performance. Record frame age as well as inference time.
- If one Pi cannot run the policy and localization concurrently, the upgrade order is: Pi 5/64-bit OS; Raspberry Pi AI Camera for supported/offloaded perception; then Jetson Orin Nano if dense depth/segmentation and SLAM must run together. NVIDIA documents 7-25 W modes and up to 67 INT8 TOPS for the current Orin Nano developer kit ([official guide](https://docs.nvidia.com/jetson/orin-nano-devkit/user-guide/latest/)).

### 3. Floor-plan localization and routing

Convert the house plan into a metric 2D occupancy grid: wall/free/unknown pixels, known scale, origin, and robot-radius inflation. Plan a global route with A* (or Dijkstra), reduce it to waypoints, and feed only the next waypoint's robot-relative bearing and distance into the learned local policy. Overlay live obstacles separately; never assume the plan includes furniture.

#### What one image can and cannot tell us

The desired state is the robot-base pose `SE(2) = (x, y, yaw)` in floor-plan coordinates. A single monocular frame has no motion parallax, and a bare floor plan supplies walls rather than the textured 3D landmarks visible in the image. Metric depth is also ambiguous unless scale comes from a known-size object, calibrated landmark, depth/stereo, or temporal motion. Most importantly, two similar corridors or rooms can produce equally plausible observations. Direct floor-plan systems therefore construct a probability volume over many `(x, y, yaw)` candidates; even newer work explicitly reports multimodal heatmaps caused by similar geometry ([PALMS+ paper](https://arxiv.org/abs/2511.09724)).

Consequently, the localizer must not return only an argmax pose. Its control-facing output should contain:

- timestamped `(x, y, yaw)` and the camera-to-base transform used;
- covariance **and**, while globally ambiguous, the top pose hypotheses or full grid/particle belief;
- observation likelihood, effective particle count or equivalent confidence, and tracking/relocalizing/lost state;
- map version and transform so a delayed server result cannot be applied to the wrong frame.

Motion supplies information: wheel/visual odometry predicts how every hypothesis moves, then each new image reweights them. Several seconds of diverse motion are usually more informative than repeatedly examining one stationary frame. The robot should slow or stop when the belief is broad or multimodal, and may deliberately rotate or move to seek a doorway/tag that distinguishes the hypotheses.

#### Using furniture and other semantic landmarks

**Yes: a detailed semantic plan can support rough—and sometimes metric—`(x,y,yaw)` localization from camera images.** The practical implementation is not a VLM directly guessing coordinates. It is a calibrated observation model inside a histogram or particle filter:

1. Detect mapped entities such as doors, windows, a wall-mounted TV, fireplace, built-in cabinets, couch and table; estimate their image bearing and, when depth is available, range.
2. For every candidate floor-plan pose, predict which mapped entities should be visible and at what bearings/ranges, accounting for walls and camera field of view.
3. Score agreement while explicitly allowing missed detections, false detections and uncertain instance association; update the full belief over candidate poses.
4. Propagate the belief between frames using visual-inertial odometry, IMU plus a learned command-motion model, or UWB position corrections. Return pose hypotheses and uncertainty, not only the winning pose.

This is established semantic localization rather than an LLM heuristic: Atanasov et al. localize against a prior map of labeled landmarks with an observation model that handles missed detections, false alarms and data association ([paper](https://doi.org/10.1177/0278364915596589)); Zimmerman et al. combine RGB semantic cues extracted from a sparse floor plan with 2D lidar in Monte Carlo localization ([paper and code](https://arxiv.org/abs/2210.01456)). A Scoutbot-only system could substitute RGB-D structural rays for lidar, but using category detections alone would be much less constrained.

The map needs more than the words “couch” and “TV.” Use metric scale and origin; walls/openings and room polygons; one record per landmark with stable instance ID, class, 2D footprint/center, approximate height and orientation; and the calibrated camera intrinsics plus camera-to-base transform. Also store `static/permanent`, last-verified time and position uncertainty. Prefer doors, windows, fireplaces, sinks, built-ins and wall-mounted objects. Treat couches and heavy tables as medium-confidence evidence, and chairs, lamps, bins, pets and people as dynamic clutter. Several separately identifiable, non-collinear landmarks are far better than repeated generic objects.

Observability sets the ceiling. A category-only sighting such as “a couch somewhere to the left” usually constrains a region and orientation, not a unique pose. One known landmark with bearing but no range leaves a continuum of possible positions; monocular apparent size gives weak range unless its dimensions and visible face are known. Multiple instance-level bearings, depth/RGB-D ranges, wall geometry, or motion over several frames can resolve the pose. Repeated furniture, occlusion, symmetry and stale object locations create multiple valid modes, so the controller should receive `localized/tracking/ambiguous/lost`, covariance and the top hypotheses. IMU helps propagate yaw but does not make an ambiguous image unique; UWB contributes absolute `x,y` but needs vision/IMU for heading.

There are four realistic implementation levels:

- **Semantic rays against the plan:** predict wall/depth rays plus door/window classes and score them over an `x-y-yaw` grid. Semantic Rays demonstrates that adding doors, windows and optional room labels reduces geometry-only ambiguity and outputs a structural-semantic probability volume ([ICCV 2025 paper](https://openaccess.thecvf.com/content/ICCV2025/html/Grader_Supercharging_Floorplan_Localization_with_Semantic_Rays_ICCV_2025_paper.html)). Its released task does not directly cover arbitrary furniture, but it is the closest ready-made formulation.
- **Georeferenced visual survey:** once, drive or walk the house and save calibrated images/RGB-D frames with floor-plan pose. At runtime, retrieve nearby keyframes, match local features, and estimate pose from 2D-to-3D correspondences with PnP/RANSAC. The official HLoc pipeline implements retrieval, feature matching, a reference SfM model and camera pose estimation ([repository](https://github.com/cvg/Hierarchical-Localization)). This is likely more reliable than hand-entering furniture because it captures texture and exact geometry; the reference map must be aligned to the floor plan.
- **Render-to-image matching:** extrude the plan into a 3D reference model and render candidate semantic/depth panoramas. SPVLoc localizes an RGB image against rendered panoramas from an untextured 3D room model with doors/windows and estimates a relative 6D pose ([ECCV 2024 project](https://fraunhoferhhi.github.io/spvloc/)). Furniture can add discriminative structure only if its 3D size, height, orientation and continued location are modeled; flat top-view icons are insufficient for photometric rendering.
- **Semantic 3D map/scene graph:** VLMaps, ConceptGraphs, HOV-SG and Hydra can attach language/object concepts to spatially grounded maps, making queries such as “between the sofa and TV” useful ([VLMaps](https://arxiv.org/abs/2210.05714), [ConceptGraphs](https://arxiv.org/abs/2309.16650), [HOV-SG](https://arxiv.org/abs/2403.17846), [Hydra](https://arxiv.org/abs/2201.13360)). ConceptGraphs also demonstrates particle-filter localization by matching current detections to mapped 3D object nodes, but that experiment is in simulation. These systems generally build from posed RGB-D/visual-inertial observations and assume or jointly maintain a geometric trajectory; they are useful after a house scan, not drop-in single-image localizers against a hand-annotated 2D plan.

A plain VLM is still useful as a coarse cue—`living room; TV ahead; couch right`—or to propose landmark matches. It should seed or reweight a calibrated filter, not emit authoritative map coordinates. For Scoutbot, first annotate permanent architectural landmarks and a few stable furniture instances, record georeferenced panoramic/keyframe imagery, and run image retrieval plus semantic landmark scoring in shadow mode. Then add RGB-D range and camera+IMU visual-inertial motion, retaining UWB as an optional global `x,y` correction. Evaluate room accuracy, heading error, metric position error, convergence time and false-confident relocalizations after furniture and lighting changes before using the estimate for autonomous driving.

#### Practical approaches

| Approach | What it needs | What it returns | Main tradeoff |
|---|---|---|---|
| **AprilTags + wheel odometry + IMU** | Calibrated camera intrinsics/extrinsics; known tag size and each tag's floor-plan pose; encoders and a low-cost IMU | A metric 6-DoF camera fix when a tag is visible, reduced to robot `(x,y,yaw)` and fused continuously with odometry | Cheapest dependable baseline and unambiguous global relocalization, but requires visible installed markers. AprilTag's official estimator uses known physical size and intrinsics and can return two planar-pose candidates plus object-space error, so gate and fuse observations rather than snapping to every detection ([pose API](https://github.com/AprilRobotics/apriltag/blob/master/apriltag_pose.h), [official repository](https://github.com/AprilRobotics/apriltag)). |
| **RGB-D/stereo/visual-inertial SLAM aligned to the plan** | A depth/stereo camera or camera+IMU, temporal frames, calibration, and an initial survey/visual map | Smooth metric 6-DoF pose, trajectory and visual map; project to `(x,y,yaw)` with uncertainty | Strong everyday tracker without markers in every view, but this does **not** localize against the drawing by itself. The SLAM map must be registered once to floor-plan coordinates using surveyed tags, dock pose, or corresponding wall points; then the saved visual map must support relocalization. ORB-SLAM3 supports monocular, stereo, RGB-D and visual-inertial modes ([paper](https://arxiv.org/abs/2007.11898)); RTAB-Map supports RGB-D, stereo and lidar graph SLAM with appearance loop closure ([official API](https://github.com/introlab/rtabmap/blob/master/doxygen/mainpage.md)). Pure monocular SLAM has an arbitrary scale; stereo/RGB-D or inertial/wheel constraints are preferable indoors. |
| **Reference-image / topological localization** | Drive each route once and store ordered keyframes, ideally tagged with floor-plan coordinates and headings | Nearest place/keyframe or graph node, match score, then a local relative waypoint rather than an inherently precise metric pose | Simple and well matched to NoMaD/ViNT navigation. NetVLAD is designed for image place retrieval ([paper](https://arxiv.org/abs/1511.07247)); ViNT consumes current/past observations and predicts temporal distance/actions to an image goal ([project](https://general-navigation-models.github.io/vint/)). Sequence matching improves over a single image ([SeqSLAM implementation and paper](https://openslam-org.github.io/openseqslam.html)), but perceptual aliasing, lighting changes, route reversal and off-route starts remain concerns. Metric `(x,y,yaw)` requires georeferenced keyframes plus interpolation/odometry. |
| **Direct image-to-floor-plan localization** | Metric raster plan; RGB image sequence; gravity, intrinsics and relative frame poses for the strongest variants; optional doors/windows/room labels | A multimodal `x-y-yaw` probability volume maintained by a histogram/particle filter | No site-specific photo survey is the attraction. F3Loc predicts horizontal depth rays from single and multiple views and filters them in SE(2) ([paper](https://arxiv.org/abs/2403.03370), [official code](https://github.com/felix-ch/f3loc)). **UnLoc** is the strongest current learned candidate: it adds per-ray depth uncertainty before exhaustive floor-plan matching and maintains a multimodal SE(2) posterior; official Gibson-trained checkpoints are released ([ICLR 2026 paper](https://arxiv.org/html/2509.11301), [official code](https://github.com/matthias-wueest/UnLoc)). Semantic Rays adds predicted wall/window/door rays and optional room labels to reduce structural ambiguity ([ICCV paper/project](https://tau-vailab.github.io/SemRayLoc/)). PALMS+ builds scale-aligned structure from a stationary rotating RGB scan and is better viewed as periodic global relocalization than a per-frame tracker ([WACV 2026 paper](https://arxiv.org/abs/2511.09724)). All remain server-side experiments until real-house domain shift and false-confident fixes are measured. |
| **Structural/semantic matching + particle filter** | Odometry plus observed wall/layout edges, RGB-D/2D lidar scans, or detected semantic landmarks that also exist on the plan | Persistent multimodal particle belief over `(x,y,yaw)` | A practical version projects RGB-D into a horizontal pseudo-laser scan and runs Monte Carlo localization directly against the occupancy-grid floor plan; Nav2 AMCL implements adaptive MCL with beam/likelihood-field sensor models ([official AMCL docs](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/others/configuring_amcl/)). A monocular layout-edge network has also been matched to plans in a particle filter ([IROS paper](https://arxiv.org/abs/1903.01804)), while another system fuses camera semantics with 2D lidar against sparse CAD plans ([paper](https://arxiv.org/abs/2210.01456)). Permanent structure tolerates furniture better than appearance matching, but glass, plan mismatch and long symmetric corridors remain difficult. |
| **UWB anchors + robot tag** | At least three surveyed anchors for a constrained 2D solve—four or more preferred for redundancy and coverage—and a UWB tag at known height on the robot | Absolute ranges and a global `(x,y)` fix in floor-plan coordinates; one tag does not observe yaw | A serious option when appearance is unreliable, not merely a fallback. It works in darkness and does not require a visual map, but anchor geometry, synchronization/range bias, walls/people and non-line-of-sight multipath affect the fix. Fuse it with wheel odometry, IMU and camera/VIO rather than driving from raw UWB positions. |

#### Short-range emitter/receiver localization

**UWB is the most practical radio version for Scoutbot.** Mount fixed anchors at measured floor-plan coordinates and one transceiver/tag on the robot. Each range constrains the tag to a circle around an anchor; three non-collinear ranges are the mathematical minimum for a 2D fix with known tag height, while a fourth or additional anchors let the estimator reject a bad range and avoid dropouts. Put anchors high, around rather than on one side of the operating area, and never collinear. The vendor's deployment guidance likewise emphasizes surrounding geometry, line of sight, separation from metal, and the geometric dilution of precision caused by a straight-line layout ([Pozyx anchor-placement guide](https://www.pozyx.io/pozyx-academy/where-to-place-the-anchors)). Interior walls may require anchors in multiple rooms.

A single robot tag provides position but no heading. Get yaw from the IMU/encoders and visual tracking; two well-separated robot tags could geometrically infer heading, but the short baseline on a small chassis amplifies range error. Feed individual ranges—or a position fix with empirically measured covariance—into the same EKF/factor graph as wheel odometry, IMU and VIO. UWB supplies the bounded global correction while those temporal sensors supply smooth motion and heading. Reject or down-weight non-line-of-sight ranges instead of allowing a confident position jump. This combination has direct research precedent: UVIO tightly integrates multiple anchor ranges and bias estimates with VIO and reports elimination of long-term position/heading drift while anchors are in range ([IROS paper](https://arxiv.org/abs/2308.00513)).

Two related emitter/receiver choices are less attractive here:

- **Bluetooth LE direction finding** uses phase/IQ samples and switched antenna arrays to estimate angle of arrival or departure ([Bluetooth Core specification](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Core-60/out/en/architecture%2C-change-history%2C-and-conventions/architecture.html), [Bluetooth SIG overview](https://www.bluetooth.com/learn-about-bluetooth/feature-enhancements/direction-finding/)). Tags can be cheap, but the fixed side needs calibrated antenna arrays, and indoor angular estimates still suffer from reflections; it is better for room/zone or coarse position unless a specific array is validated in the house.
- **Ultrasound + RF time-of-flight** can be very accurate at room scale: MIT's Cricket system used ceiling beacons, concurrent RF/ultrasound pulses and a mobile listener, reporting 5 cm ranging and 10 cm position in its research deployment ([MIT thesis](https://dspace.mit.edu/entities/publication/a676b91a-d332-4741-8838-e234b5535915)). It requires dedicated acoustic hardware and relatively clear propagation; reflections, occlusion, beacon interference and slower update dynamics make it less convenient across a furnished multi-room house than UWB.

#### Recommended Scoutbot progression

1. **Create one coordinate system.** Redraw/rasterize one floor at verified metric scale; choose origin and axes; measure the camera-to-base transform. Calibrate camera intrinsics. Every log and service response should use the same named map frame.
2. **Build the reference baseline.** Add encoders and IMU, place tags at the dock, corridor transitions and visually repetitive junctions, and fuse odometry with tag corrections in an EKF/UKF or particle filter. Preserve pose covariance and reject tag outliers. This is both the first usable localizer and a measured reference trajectory for comparing less-instrumented methods; call it “ground truth” only after independently surveying its error.
3. **Evaluate UWB if installed anchors are acceptable.** Survey at least four anchors around the test area, put one tag on the robot, and fuse its ranges/position with the same encoder/IMU/camera state estimator. Measure the error distribution through walls and around people before assigning its covariance. UWB can replace most visual markers for position, but not the yaw source.
4. **Add RGB-D floor-plan matching and visual SLAM.** First test a horizontal depth scan with AMCL directly against the metric plan. Also survey a saved RGB-D/stereo SLAM map, register it to the plan using multiple tags/correspondences, and test relocalization from cold starts. RGB-D also supports the independent obstacle layer, so it provides more value than a second monocular camera experiment.
5. **Record a visual topological map at the same time.** Georeference route keyframes with the fused baseline. This directly supports NoMaD/ViNT-style image-goal navigation and offers a coarse fallback (`room/node + confidence`) if metric tracking is lost.
6. **Benchmark learned floor-plan models offline.** Use F3Loc to validate the data/plan pipeline, then compare UnLoc as the stronger uncertainty-aware baseline; add Semantic Rays if the plan can be labeled with doors/windows/room types, and PALMS+ for stop-and-spin relocalization. Compare full pose beliefs—not only best poses—against the tagged/UWB reference. Adopt one only after real-house tests establish convergence time, false confident fixes, and recovery across lighting/furniture changes.

If adopting ROS 2 later, Nav2 already supplies this modular split: AMCL localizes in a static map ([official docs](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/others/configuring_amcl/)), and an RGB-D image can be converted to a 2D laser scan for navigation/localization ([ROS package docs](https://docs.ros.org/en/ros2_packages/humble/api/depthimage_to_laserscan/index.html)). ROS is optional for the first prototype; its value rises once mapping, transforms, recovery behaviors, and sensor fusion outgrow a small process graph.

## Robustness options and recommended stack

Robust navigation is not one better model. It is a set of independent layers in which a stale server result, lost camera track, bad UWB range, blocked route, or crashed process causes a bounded recovery or stop rather than continued motion.

```text
VLM/VLA (optional): destination or candidate short trajectory
                              |
floor plan -> global path -> deterministic local controller (MPPI/DWB)
                              |
                       velocity smoother
                              |
live lidar/depth/ToF -> independent collision supervisor
                              |
             leased Axum command -> Pi watchdog -> MCU watchdog -> motors

camera+IMU VIO ---------------------> continuous odom pose
2D lidar AMCL + UWB/tags -----------> global map correction + uncertainty
```

### Priority by expected value

| Priority | Option | Impact | Cost | Integration complexity | Why it matters |
|---|---|---:|---:|---:|---|
| 1 | Active stop, command TTL/sequence, and MCU watchdog | Very high | Low | Low-medium | The current Axum task repeats a byte indefinitely, `/stop` does not transmit `s`, and UDP/serial has no freshness or acknowledgement. Every autonomous command should expire locally even if the GPU server, network, Axum, Python bridge, or Pi fails. |
| 2 | Bumper ring, downward cliff sensors, physical E-stop, front/rear short-range ToF | Very high | Low-medium | Low-medium | These are independent of scene understanding. A bumper is the last-resort contact stop; dedicated downward sensors protect stairs; rear sensing is required before autonomous reverse. |
| 3 | 360-degree 2D lidar and a live local costmap | Very high | Medium | Medium | This is the most mature indoor geometry layer for wall localization and collision avoidance. Nav2 AMCL localizes a robot in a static map with a laser scan and odometry ([AMCL](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/others/configuring_amcl/)). A single scan plane can miss low/overhanging objects and can struggle with some glass or dark surfaces, so it complements rather than replaces depth, ToF and bumpers. |
| 4 | Hardware-synchronized stereo/RGB-D camera plus IMU and VIO | High | Medium-high | Medium-high | It supplies local motion and heading without wheel encoders and observes obstacles outside a lidar plane. Nav2 documents VIO as an augmentation or replacement for absent wheel odometry and warns that camera/IMU synchronization is central to practical quality ([VIO integration](https://docs.nav2.org/rolling/tutorials/general_tutorials/integrating_vio/integrating_vio/)). |
| 5 | UWB and/or surveyed AprilTags | High | Medium | Medium | These give absolute corrections and cold-start recovery. UWB constrains `x,y` but not yaw with one tag; camera tags can correct full pose when visible. Keep both as measurements with empirical covariance, not truth that instantly overwrites the estimate. |
| 6 | Continuous velocity interface and deterministic path controller | High | Low-medium hardware; medium software | Medium-high | Five discrete actions create oscillation and make path tracking, stopping distance and acceleration limiting difficult. A local controller can choose smooth `vx`, optional `vy`, and `wz` while checking the robot footprint. |
| 7 | VLA/VLM semantic assistance | Medium | GPU server | High | Useful for selecting a named goal, interpreting a room, or proposing a short trajectory. It should never bypass localization health, costmap collision checks, speed limits or the command lease. |

If only one substantial sensor can be added, choose **2D lidar** for the most conventional indoor stack. If the no-encoder requirement is firm, choose a **stereo/RGB-D camera with an integrated, hardware-synchronized IMU** next; an ordinary RGB stream plus an unrelated USB IMU is a much harder VIO system to make dependable. The strongest encoder-free configuration is lidar + synchronized camera/IMU VIO + one absolute correction source, with bumper/cliff/range sensors independently wired into stopping.

### Localization and planning responsibilities

Maintain the conventional `map -> odom -> base_link` separation:

- `odom -> base_link` must be smooth and high-rate. Without encoders, obtain it from VIO + IMU. It may drift, but must not jump.
- `map -> odom` supplies global correction from lidar AMCL, registered VSLAM, UWB, AprilTags, or semantic floor-plan localization. It may adjust as evidence arrives.
- Fuse timestamped measurements with covariance and outlier rejection. Publish `tracking`, `ambiguous`, `relocalizing`, or `lost`, the age of the newest observation, and position/yaw uncertainty. Do not average two contradictory global poses into a confident-looking midpoint. `robot_localization` provides EKF/UKF state estimation, per-input field selection, sensor timeouts, rejection thresholds, and separate continuous/global-frame configurations ([official documentation](https://docs.ros.org/en/kinetic/api/robot_localization/html/state_estimation_nodes.html)).

Use the detailed house plan as the static occupancy layer, inflated by the robot footprint, and overlay current lidar/depth observations in a rolling local costmap. Keep-outs should include stairs, fragile furniture margins, cables and areas the robot cannot safely traverse. Let A*/Smac produce the global route; use **DWB** initially for a simple differential/omnidirectional velocity-sampling controller, or **MPPI** on the GPU/CPU server when smoother predictive behavior is worth the tuning. DWB scores footprint collision, path/goal alignment and oscillation, while MPPI forward-simulates batches of controls and supports differential, omni and Ackermann motion models ([DWB](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/controller_plugins/dwb_controller/), [MPPI](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/controller_plugins/mppi_controller/configuring_mppic/)).

Replace raw repeated direction bytes for autonomy with stamped, bounded commands such as `{sequence, issued_at, ttl_ms, vx, vy, wz}`. Axum should validate ranges and monotonically increasing sequence numbers; the Pi should stop on expiration or server disconnect; the Arduino should independently stop if no valid refresh arrives. During migration, bounded primitives such as `rotate(angle, max_speed, timeout)` and `advance(distance, max_speed, timeout)` are safer than indefinite `forward`, but continuous velocity commands are the final interface. Nav2's Velocity Smoother is a useful reference: it enforces acceleration/deceleration/deadband limits and sends zero velocity after a timeout; without odometry it can run open-loop, but that represents commanded rather than measured speed ([official docs](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/core_servers/configuring_velocity_smoother/)).

Place an independent collision supervisor after smoothing and before the hardware gateway. It should consume fresh lidar/depth/ToF data and choose the most conservative of stop, slow, or velocity-limit zones. Nav2 Collision Monitor follows this pattern by bypassing the planner/costmap and stopping when observation data is stale, but its documentation makes clear that CPU-level monitoring is not hard-real-time safety equipment; the MCU watchdog, physical stop and direct bumper/cliff path remain necessary ([official docs](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/core_servers/collision_monitor/configuring_collision_monitor_node/)).

### Recovery, calibration and validation

Make recovery explicit and bounded. When pose uncertainty or frame age crosses a threshold: stop, rotate slowly to seek lidar geometry/tags/visual landmarks, request global relocalization, and continue only after one hypothesis dominates. If blocked: wait, replan, clear only transient local obstacles, then try one short supervised backup only with rear clearance. Nav2 behavior trees provide periodic replanning and retry-limited contextual/system recoveries, and AMCL exposes global relocalization for severe delocalization ([BT documentation](https://docs.nav2.org/rolling/getting_started/nav2_behavior_trees/), [global relocalization](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/core_servers/bt_plugins/actions/ReinitializeGlobalLocalization/)).

Calibration is part of the product: measure camera intrinsics, camera/IMU/lidar/UWB-to-base transforms, floor-plan scale/origin, motor deadbands and the full camera-to-actuator delay. Timestamp at sensor capture, synchronize clocks between Pi and GPU server, and match camera/IMU/lidar by acquisition time rather than arrival time; ROS explicitly requires synchronized system clocks for distributed timestamped data, and its message filters support approximate-time alignment ([ROS clock design](https://design.ros2.org/articles/clock_and_time.html), [message synchronization](https://docs.ros.org/en/ros2_packages/jazzy/api/message_filters/doc/Tutorials/Approximate-Synchronizer-Cpp.html), [OpenCV camera calibration](https://docs.opencv.org/4.13.0/d4/d94/tutorial_camera_calibration.html)).

Record synchronized frames, raw sensors, pose beliefs, costmaps, planned paths, model outputs, commands, acknowledgements and interventions. Replay every software release against fixed logs; test disconnects, frozen frames, delayed/out-of-order responses, bad UWB ranges and localization jumps. Use Nav2's lightweight Loopback Simulator for fast behavior-tree/controller integration tests, then Gazebo or another physics/sensor simulator for dynamics and sensing; Loopback intentionally assumes a perfect frictionless command-to-odometry model and therefore cannot validate localization or stopping distance ([Loopback Simulator](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/others/configuring_loopback_sim/)). Track route completion, interventions per meter, minimum obstacle clearance, false-forward events, relocalization time, p95 sensor-to-command age and stopping distance at each speed.

Recommended rollout: **(1)** fix active stop/leases/MCU watchdog and add bumper/cliff sensing; **(2)** adopt stamped continuous velocity and acceleration limits; **(3)** add lidar with a one-room occupancy map and deterministic controller; **(4)** add synchronized camera+IMU VIO as encoder-free odometry, then UWB/tags for global correction; **(5)** add behavior-tree recovery, record/replay and fault injection; **(6)** only then compare NoMaD/OmniVLA or another VLA as a semantic/subgoal or candidate-trajectory layer under the same deterministic safety envelope.

## Controller and safety changes required before autonomous motion

The model should propose movement; a deterministic supervisor should authorize it.

- Replace indefinite `/move/:direction` behavior for autonomous callers with a validated command such as `{action, sequence, issued_at, ttl_ms}`. Require refresh at 5-10 Hz and send stop when the lease expires, the camera is stale, localization is lost, inference fails, or the client disconnects. Add the final watchdog in the Arduino/motor controller so it survives a Pi or network failure.
- Stop must preempt immediately. Apply confidence gating, action hysteresis, a low speed cap, and a maximum continuous-action duration. Drop low-confidence output to stop, not to the previous command.
- Keep collision protection independent from the learned model: bumper plus short-range ToF/depth/2D lidar, robot-footprint checks, and a physical emergency stop. Nav2's Collision Monitor illustrates the pattern—fresh sensor data can bypass planning to stop or slow the robot—but its own documentation explicitly says CPU-level monitoring is not hard-real-time safety certification ([official docs](https://docs.nav2.org/rolling/configuration_and_development/configuration_guide/core_servers/collision_monitor/configuring_collision_monitor_node/)).
- A front camera cannot make reverse safe. Disable learned reverse until there is rear coverage (rear camera/range sensors); initially use reverse only as a supervised, short recovery action.
- Test first with wheels raised, then in an empty bounded area at low speed with a human holding the stop control. Stairs require a dedicated downward-facing/cliff sensor; monocular depth is not an acceptable sole safeguard.

## Data and training plan

1. Extend teleoperation logging so each camera frame has a monotonic timestamp, command, command start/end, robot pose/odometry/IMU, range/depth readings, and intervention flag. Preserve **stop** periods and collect deliberate recoveries from bad headings, not only clean center-line driving.
2. Align actions to the frame that caused the human decision; compensate measured camera/UI/actuator delay. Downsample repeated identical frames and rebalance actions so `forward` does not dominate.
3. Split validation by complete run and room, not random neighboring frames. Report per-class precision/recall, especially “predicted forward when expert says stop,” plus closed-loop interventions, collisions/near misses, route completion, oscillations, frame age, and p95 latency.
4. Augment exposure, color, blur, shadows, and small camera shifts. A horizontal flip is valid only if left/right labels and every map-conditioned feature are swapped too.
5. Train by behavior cloning, run in shadow mode, then collect human corrections on states induced by the model. This is the DAgger pattern for addressing compounding imitation errors ([original paper](https://arxiv.org/abs/1011.0686)). Keep autonomous data collection slow and supervised.

## Staged prototype

**Stage 0 — actuator safety:** define the exact motor semantics; validate the action enum; add TTL/sequence/watchdogs, immediate stop, telemetry, and a hardware kill path.

**Stage 1 — shadow classifier:** log teleoperation data, train MobileNetV3-Small INT8, and run it beside the operator without actuating. Gate release on held-out-room results and target-device latency, not just aggregate accuracy.

**Stage 2 — bounded reactive drive:** allow forward/yaw/stop only in an empty test area. The safety layer vetoes motion; the operator remains the oracle and generates corrective examples.

**Stage 3 — mapped waypoint drive:** rasterize one floor, install a few AprilTags, fuse encoder/IMU/tag pose, plan to short nearby goals, and condition the policy on the next waypoint. Add rear sensing before enabling reverse.

**Stage 4 — reduce infrastructure:** compare ORB-SLAM3/RGB-D localization with direct floor-plan models such as F3Loc, using the tag-based system as ground truth. Keep tags as recovery anchors unless experiments show that removing them preserves relocalization and safety margins.

The first useful milestone is therefore not “a model drives the whole house.” It is: **the robot predicts five actions at 10 Hz in shadow mode, the controller always stops on stale input, and a tagged one-room map reports pose with measured uncertainty.** That milestone creates the data and safety foundation for everything more ambitious.
