# Semantic navigation options for Scoutbot

## Decision

ESC, LM-Nav, VLMaps, and Pix2Map should not replace lidar/IMU odometry, metric localization, Nav2 planning, or the independent collision-stop layer. For Scoutbot, use lidar + IMU as the geometric navigation backbone and put semantic or topological reasoning above it:

```text
language instruction
       |
allowlisted intent parser
       |
room/object phrase -> known metric goal or surveyed topological node
       |
Nav2 localization, path planning, obstacle avoidance, and velocity control
       |
watchdog + bumper/cliff/collision stop -> motors
```

The best first implementation is a hand-labeled goal map for `kitchen`, `hallway`, and `charger_staging`. A small LM-Nav-style visual graph is a useful second experiment. VLMaps becomes relevant after adding RGB-D and reliable camera poses. ESC is for searching for objects whose location is unknown. Pix2Map is not an indoor navigation solution for this robot.

## Naming distinction

- **ESC (Exploration with Soft Commonsense Constraints)** is zero-shot object-goal exploration. It uses RGB-D observations, pose/map state, visual grounding, LLM-derived room/object priors, and frontier exploration. It is not a general language route follower or a topological controller. Its project page says the source code cannot be publicly released ([official project](https://sites.google.com/ucsc.edu/escnav/home), [paper](https://arxiv.org/abs/2301.13166)).
- **LM-Nav** is the closer match to an LLM-based topological navigator. It builds a graph from previously collected visual observations, turns instructions into landmark sequences, grounds landmarks to graph images, and executes the route with a learned visual navigation model ([official project](https://sites.google.com/view/lmnav), [code](https://github.com/blazejosinski/lm_nav), [paper](https://arxiv.org/abs/2207.04429)).

## Comparison

| System | Required inputs | Produces | Scoutbot fit |
|---|---|---|---|
| ESC | RGB-D, pose/map state, target object class, semantic detections | Preferred frontier or exploration goal | Later option for “find a mug” in an unknown location. It still needs geometric mapping and local navigation. |
| LM-Nav | Language instruction, RGB observations, previously surveyed visual graph, learned visual navigator | A route through visual graph nodes | Relevant for landmark-rich route instructions. For named rooms, metric goals are simpler and safer. Use Nav2 to execute graph-node poses. |
| VLMaps | Posed RGB-D survey frames and camera calibration | Open-vocabulary relevance over spatial map cells | Good later semantic-goal layer. The current RGB camera plus planar lidar does not provide the dense per-pixel depth expected by the released pipeline ([repository](https://github.com/vlmaps/vlmaps), [paper](https://arxiv.org/abs/2210.05714)). |
| Pix2Map | Ego-view road images and candidate street graphs | Retrieved/inferred outdoor road topology | Not applicable as a live indoor localization or control stack. The published system targets urban street maps and Argoverse-style data ([official project](https://pix2map.github.io/), [paper](https://arxiv.org/abs/2301.04224)). |

## Recommended architecture

Maintain a semantic file beside the lidar occupancy map:

```yaml
kitchen:
  type: region
  approach_poses: [[x1, y1, yaw1], [x2, y2, yaw2]]
hallway:
  type: region
  approach_poses: [[x3, y3, yaw3]]
charger:
  type: staging_pose
  pose: [x4, y4, yaw4]
```

Expose only validated actions:

- `navigate_named_goal(name)` for rooms and the manual-docking staging position;
- `describe_latest_frame()` for “tell me what you see,” normally while stopped;
- `stop()` as a local, preemptive action.

The LLM may resolve synonyms and select an existing target. It must not invent coordinates or issue motor commands. Nav2 still requires smooth odometry and the standard transforms, but odometry can come from lidar, VIO, IMU, or encoders rather than specifically from wheel encoders ([Nav2 state estimation](https://docs.nav2.org/rolling/getting_started/navigation_concepts/state_estimation/)).

## Staged evaluation

1. Build the lidar/IMU geometric baseline and validate safe autonomous travel to fixed metric poses.
2. Label kitchen, hallway, and charger staging poses and expose them through a constrained language router.
3. During manual surveys, save RGB keyframes with their metric poses. Build a small visual/topological graph whose nodes retain those poses.
4. Test an LM-Nav-style semantic node selector in shadow mode; let Nav2 travel to the selected node.
5. Add RGB-D only if open-vocabulary goals such as “near the plant” justify VLMaps' complexity.
6. Add ESC-like semantic frontier ranking only when unknown-object search becomes a real requirement.

Evaluate semantic selection separately from motion safety: target correctness, reachability of the selected goal, route completion, interventions, clearance, localization loss, and stale-response stopping.
