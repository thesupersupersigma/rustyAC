//! `DragMode` (update, finish line, new session, goals) on made-up matches against
//! `rustyac_physics::race::drag`, every member, call and lap every frame.

use super::*;
use rustyac_physics::race::drag::{DragFrame, DragHost, DragMode, DragState, DragStatus};

const VA_DRAG_VTABLE: usize = 0x1_404b_31e0;
const VA_DRAG_UPDATE: usize = 0x1_4008_8c30;
const VA_DRAG_FINISH: usize = 0x1_4008_70e0; // onFinishLanePassed(const unsigned &)
const VA_DRAG_NEW_SESSION: usize = 0x1_4008_5a60; // the constructor's lambda on OnNewSession
const VA_DRAG_GENERATE_START: usize = 0x1_4008_6ea0;
const VA_SPLINE_CTOR: usize = 0x1_401e_ecd0;
const VA_SPLINE_ADD_POINT: usize = 0x1_401e_f700;
const VA_SPLINE_LENGTH: usize = 0x1_401f_1db0;
const VA_SPLINE_SIGNED: usize = 0x1_401f_2df0;
const VA_SPLINE_WORLD_TO: usize = 0x1_401f_3bf0;
const VA_GO_TO_SPAWN: usize = 0x1_400d_3ae0;
const VA_RESET_TRANSPONDER: usize = 0x1_400d_9b70;
const VA_GET_SEMAPHORE: usize = 0x1_4008_4960; // GameObject::getGameObject<StartingTrackSemaphore>
const VA_USE_MPH: usize = 0x1_4155_a5ae; // Speed::useMPH
const DRAG_SIZE: usize = 0x2f0;
const DRAG_LAP_TIMES: usize = 0xc8;
const DRAG_CARS: usize = 0xd8;
const DRAG_STATUS: usize = 0xf0;
const DRAG_SIM: usize = 0xf8;
const DRAG_MATCHES: usize = 0x110;
const DRAG_START: usize = 0x118;
const DRAG_NULL_COUNTER: usize = 0x120;
const DRAG_SPLINE: usize = 0x270;
const DRAG_PERC: usize = 0x2e0;
const DRAG_COUNT_DOWN: usize = 0x2e8;
const CA_GUID: usize = 0x1158;
const CA_BODY_POSITION: usize = 0x254;
const CA_STATE_POSITION: usize = 0x29c;
const CA_DRAG_SPEED: usize = 0x4d8;
const SIM_CARS: usize = 0x208;

#[derive(Default)]
struct DragWorld {
    replay: bool,
    log: Vec<String>,
    semaphore: usize,
    rand: usize,
}

thread_local! {
    static DRAG: RefCell<DragWorld> = RefCell::new(DragWorld::default());
}

extern "C" fn drag_is_in_replay(_replay: *mut u8) -> bool {
    DRAG.with(|d| d.borrow().replay)
}
extern "C" fn drag_gentle_stop(car: *mut u8, on: bool) {
    let i: i32 = unsafe { rd(car, CA_GUID) };
    DRAG.with(|d| d.borrow_mut().log.push(format!("gentle {i} {on}")));
}
extern "C" fn drag_go_to_spawn(car: *mut u8, _set: *const u8) {
    let i: i32 = unsafe { rd(car, CA_GUID) };
    DRAG.with(|d| d.borrow_mut().log.push(format!("spawn {i}")));
}
extern "C" fn drag_nothing(_this: *mut u8) {}
extern "C" fn drag_semaphore(_sim: *mut u8) -> usize {
    DRAG.with(|d| d.borrow().semaphore)
}
extern "C" fn drag_fade_in(_cameras: *mut u8, _fade: *const u8) {
    DRAG.with(|d| d.borrow_mut().log.push("fade results".into()));
}
/// `AIDriverDrag::setStartTime` (vtable +0x60): one `rand()`, like the real one.
extern "C" fn drag_ai_start_time(ai: *mut u8, time: f64) {
    let i: usize = unsafe { rd(ai, 8) };
    let rand = DRAG.with(|d| d.borrow().rand);
    let rand: extern "C" fn() -> i32 = unsafe { std::mem::transmute(rand) };
    rand();
    DRAG.with(|d| d.borrow_mut().log.push(format!("ai {i} {:#x}", time.to_bits())));
}

struct PortDragHost {
    rand: rustyac_physics::session::MsvcRand,
    log: Vec<String>,
}

impl DragHost for PortDragHost {
    fn rand(&mut self) -> i32 {
        self.rand.next()
    }
    fn is_ai(&self, car: usize) -> bool {
        car == 1
    }
    fn ai_start_time(&mut self, car: usize, time: f64) {
        self.rand.next();
        self.log.push(format!("ai {car} {:#x}", time.to_bits()));
    }
    fn gentle_stop(&mut self, car: usize, on: bool) {
        self.log.push(format!("gentle {car} {on}"));
    }
    fn go_to_spawn(&mut self, car: usize) {
        self.log.push(format!("spawn {car}"));
    }
    fn show_esc_menu(&mut self) {}
    fn fade_in(&mut self, _seconds: f32, results: bool) -> bool {
        if results {
            self.log.push("fade results".into());
        }
        false
    }
}

fn car_line(c: &DragStatus) -> String {
    let b = |v: [f32; 3]| v.map(f32::to_bits);
    format!(
        "fs {} dq {} wins {} react {:#x} time {:#x} final {} max {} side {:#x} lowest {:#x} spawn {:x?} still {:x?}",
        c.false_start,
        c.disqualified,
        c.wins,
        c.reaction_time.to_bits(),
        c.player_time.to_bits(),
        c.final_speed,
        c.max_speed,
        c.spline_side.to_bits(),
        c.lowest_reaction_time.to_bits(),
        b(c.spawn_position),
        b(c.still_position)
    )
}

type Laps = Vec<Vec<(u32, Vec<u32>, u32, bool)>>;

/// The game's `DragMode` block in the port's shape.
unsafe fn game_side_drag(d: *const u8, semaphore: *const u8, avatars: &[*mut u8]) -> (String, Vec<String>, Laps) {
    let head = format!(
        "status {} matches {} start {:#x} null {:#x} count {:#x} perc {:#x} {:#x} sem {} {:#x}",
        rd::<u32>(d, DRAG_STATUS),
        rd::<u32>(d, DRAG_MATCHES),
        rd::<u64>(d, DRAG_START),
        rd::<u32>(d, DRAG_NULL_COUNTER),
        rd::<u32>(d, DRAG_COUNT_DOWN),
        rd::<u32>(d, DRAG_PERC),
        rd::<u32>(d, DRAG_PERC + 4),
        *semaphore.add(0x7c) != 0,
        rd::<u64>(semaphore, 0x80)
    );
    let (begin, end): (usize, usize) = (rd(d, DRAG_CARS), rd(d, DRAG_CARS + 8));
    let cars = (begin..end)
        .step_by(0x48)
        .map(|p| {
            let p = p as *const u8;
            let v3 = |at: usize| [rd::<f32>(p, at), rd(p, at + 4), rd(p, at + 8)];
            car_line(&DragStatus {
                false_start: *p != 0,
                disqualified: *p.add(1) != 0,
                wins: rd(p, 4),
                reaction_time: rd(p, 0x18),
                player_time: rd(p, 0x1c),
                final_speed: rd(p, 0x20),
                max_speed: rd(p, 0x24),
                spline_side: rd(p, 0x28),
                lowest_reaction_time: rd(p, 0x2c),
                spawn_position: v3(0x30),
                still_position: v3(0x3c),
            })
        })
        .collect();
    // the map's nodes: left +0, parent +8, right +0x10, isnil +0x19, key +0x20, vector +0x28
    let mut laps = vec![Vec::new(); avatars.len()];
    let root: *const u8 = rd(rd::<*const u8>(d, DRAG_LAP_TIMES), 8);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if *node.add(0x19) != 0 {
            continue;
        }
        stack.push(rd(node, 0));
        stack.push(rd(node, 0x10));
        let key: *mut u8 = rd(node, 0x20);
        let i = avatars.iter().position(|a| *a == key).expect("a car of the map");
        let (b, e): (usize, usize) = (rd(node, 0x28), rd(node, 0x30));
        laps[i] = (b..e)
            .step_by(0x48)
            .map(|l| {
                let l = l as *const u8;
                let (sb, se): (usize, usize) = (rd(l, 8), rd(l, 0x10));
                (rd(l, 0), (sb..se).step_by(4).map(|s| *(s as *const u32)).collect(), rd(l, 0x20), *l.add(0x24) != 0)
            })
            .collect();
    }
    (head, cars, laps)
}

fn port_side_drag(port: &DragMode) -> (String, Vec<String>, Laps) {
    let head = format!(
        "status {} matches {} start {:#x} null {:#x} count {:#x} perc {:#x} {:#x} sem {} {:#x}",
        port.status as u32,
        port.match_number,
        port.start_time.to_bits(),
        port.null_counter.to_bits(),
        port.count_down.to_bits(),
        port.player_perc.to_bits(),
        port.ai_perc.to_bits(),
        port.disqualified_on,
        port.custom_start.to_bits()
    );
    let laps = port.lap_times.iter().map(|l| l.iter().map(|l| (l.time, l.splits.clone(), l.cuts, l.is_valid)).collect()).collect();
    (head, port.cars.iter().map(car_line).collect(), laps)
}

/// Sessions of made-up drag matches on a hand-built block, cars and spline. Run last: its
/// stand-ins replace ones the other parts use. `report.drag_*` count what was compared.
pub(super) unsafe fn drag(acs: &Acs, base: &GameMode, seed: u32, next: &mut dyn FnMut() -> u32, frames: usize, report: &mut Report) -> Result<(), String> {
    let stub = |va: usize, target: usize| {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]);
        acs.patch(acs.va(va), &code);
    };
    stub(VA_ADD_MESSAGE, add_message as *const () as usize);
    stub(VA_IS_IN_REPLAY, drag_is_in_replay as *const () as usize);
    stub(VA_SET_GENTLE_STOP, drag_gentle_stop as *const () as usize);
    stub(VA_GO_TO_SPAWN, drag_go_to_spawn as *const () as usize);
    stub(VA_RESET_TRANSPONDER, drag_nothing as *const () as usize);
    stub(VA_GET_SEMAPHORE, drag_semaphore as *const () as usize);
    stub(VA_FADE_IN, drag_fade_in as *const () as usize);
    stub(VA_REGISTER_TO_FADE_IN, first_argument as *const () as usize);
    let operator_new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(acs.crt_function(c"??2@YAPEAX_K@Z"));
    let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
    let crt_rand = acs.crt_function(c"rand");
    let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GAME_MODE_CTOR));
    let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_DRAG_UPDATE));
    let finish: extern "C" fn(*mut u8, *const u32) = std::mem::transmute(acs.va(VA_DRAG_FINISH));
    let new_session: extern "C" fn(*mut usize, *const u8) = std::mem::transmute(acs.va(VA_DRAG_NEW_SESSION));
    let generate_start: extern "C" fn(*mut u8) -> f64 = std::mem::transmute(acs.va(VA_DRAG_GENERATE_START));
    let spline_ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SPLINE_CTOR));
    let add_point: extern "C" fn(*mut u8, *const [f32; 3], i32) = std::mem::transmute(acs.va(VA_SPLINE_ADD_POINT));
    let spline_length: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_SPLINE_LENGTH));
    let signed: extern "C" fn(*mut u8, *const [f32; 3], f32) -> f32 = std::mem::transmute(acs.va(VA_SPLINE_SIGNED));
    let world_to: extern "C" fn(*mut u8, *const [f32; 3], i32) -> f32 = std::mem::transmute(acs.va(VA_SPLINE_WORLD_TO));
    let ai_vtable = Box::leak(vec![0usize; 16].into_boxed_slice()).as_mut_ptr();
    *ai_vtable.add(0x60 / 8) = drag_ai_start_time as *const () as usize;

    let mut done = 0;
    let mut session = 0u32;
    while done < frames {
        session += 1;
        srand(seed.wrapping_add(session));
        let mut host = PortDragHost { rand: rustyac_physics::session::MsvcRand(seed.wrapping_add(session)), log: Vec::new() };
        let matches = [3u32, 5, 1, 0, 4][session as usize % 5];
        let use_mph = session % 4 == 1;
        acs.set_global(VA_USE_MPH, use_mph as u8);
        // the mode with goals of 1, 2, 3... wins
        let d = zeroed(DRAG_SIZE);
        let name = zeroed(0x20);
        write_wstring(name, "DRAG_MODE");
        let game = zeroed(0x278);
        ctor(d, name, game);
        wr(d, 0, acs.va(VA_DRAG_VTABLE));
        let mut base = base.clone();
        let goals: usize = rd(d, GM_CONDITIONS);
        for (i, c) in base.conditions.iter_mut().enumerate() {
            (c.objective, c.achieved) = (i as i32 + 1, false);
            wr(goals as *mut u8, i * 0x48 + 0x40, i as u32 + 1);
            wr(goals as *mut u8, i * 0x48 + 0x44, 0u8);
        }
        // the sim, the cars, the semaphore
        let sim = zeroed(0x2d0);
        let pause = zeroed(0x160);
        wr(sim, SIM_PAUSE_MENU, pause);
        wr(sim, SIM_REPLAY_MANAGER, zeroed(0x200));
        wr(sim, SIM_SYSTEM_MESSAGE, zeroed(0x100));
        let semaphore = zeroed(0x100);
        DRAG.with(|w| *w.borrow_mut() = DragWorld { semaphore: semaphore as usize, rand: crt_rand, ..DragWorld::default() });
        let avatars = [zeroed(0x1300), zeroed(0x1300)];
        let car_list = Box::leak(avatars.to_vec().into_boxed_slice()).as_ptr();
        wr(sim, SIM_CARS, car_list);
        wr(sim, SIM_CARS + 8, car_list.add(2));
        wr(sim, SIM_CARS + 0x10, car_list.add(2));
        let ai = zeroed(0x20);
        wr(ai, 0, ai_vtable);
        wr(ai, 8, 1usize);
        // the track: the start slots and the finish's nodes
        let mut jitter = || (next() % 100) as f32 * 0.01;
        let spawns = [[-2.0 - jitter(), jitter() * 0.1, jitter()], [2.0 + jitter(), jitter() * 0.1, jitter()]];
        let length = [201.0f32, 402.3, 99.7][session as usize % 3];
        let finish_nodes = [[-6.0 + jitter(), 0.0, length], [6.0 + jitter(), 0.0, length + jitter()]];
        // the game constructor's part
        wr(d, DRAG_STATUS, 3u32);
        wr(d, DRAG_SIM, sim);
        wr(d, DRAG_MATCHES, matches);
        let head = operator_new(0x40);
        std::ptr::write_bytes(head, 0, 0x40);
        for at in [0, 8, 0x10] {
            wr(head, at, head);
        }
        (*head.add(0x18), *head.add(0x19)) = (1, 1);
        wr(d, DRAG_LAP_TIMES, head);
        let spline = d.add(DRAG_SPLINE);
        spline_ctor(spline);
        wr(spline, 0x6c, 0u32); // eLinear
        let s = [0, 1, 2].map(|k| (spawns[0][k] + spawns[1][k]) * 0.5);
        let dd = [0, 1, 2].map(|k| (finish_nodes[1][k] + finish_nodes[0][k]) * 0.5 - s[k]);
        for i in 0..=100u32 {
            let t = i as f32 * f32::from_bits(0x3c23_d70a);
            add_point(spline, &[dd[0] * t + s[0], dd[1] * t + s[1], dd[2] * t + s[2]], 0);
        }
        spline_length(spline);
        let statuses = operator_new(2 * 0x48);
        std::ptr::write_bytes(statuses, 0, 2 * 0x48);
        for (i, a) in avatars.iter().enumerate() {
            let c = statuses.add(i * 0x48);
            wr(*a, CA_GUID, i as i32);
            wr(*a, CA_BODY_POSITION, spawns[i]);
            wr(*a, CA_STATE_POSITION, spawns[i]);
            wr(c, 8, *a);
            if i == 1 {
                wr(c, 0x10, ai);
            }
            let n = world_to(spline, &spawns[i], -1);
            let v = signed(spline, &spawns[i], n);
            wr(c, 0x28, if v > 0.0 { 1.0f32 } else if v < 0.0 { -1.0 } else { 0.0 });
            wr(c, 0x2c, f32::MAX);
            wr(c, 0x30, spawns[i]);
        }
        wr(d, DRAG_CARS, statuses);
        wr(d, DRAG_CARS + 8, statuses.add(2 * 0x48));
        wr(d, DRAG_CARS + 0x10, statuses.add(2 * 0x48));
        let mut game_time = (next() % 100000) as f64;
        wr(game, 0x18, game_time);
        wr(d, DRAG_START, generate_start(d));
        let mut port = DragMode::new(base, matches, spawns, finish_nodes, &spawns, game_time, &mut host);

        let mut f = DragFrame { game_time, positions: spawns.to_vec(), state_positions: spawns.to_vec(), ..DragFrame::default() };
        let mut speeds = [0.0f32; 2];
        let mut react = [0.0f32; 2];
        let mut accel = [6.0f32, 7.0];
        let mut jump = [false; 2];
        let mut dq_line = [false; 2];
        let mut still_frames = 0;
        let mut lambda = [d as usize];
        let (mut session_frames, mut over_frames, mut new_sessions) = (0, 0, 0);
        loop {
            let dt = (5 + next() % 30) as f32 * 0.001;
            game_time += dt as f64 * 1000.0;
            f.game_time = game_time;
            wr(game, 0x18, game_time);
            f.paused = next() % 2000 == 0; // (a pause before the green rolls the lights again)
            f.replay = next() % 3000 == 0;
            DRAG.with(|w| w.borrow_mut().replay = f.replay);
            *pause.add(0x150) = f.paused as u8;
            DRAG.with(|w| w.borrow_mut().log.clear());
            host.log.clear();
            SAID.with(|s| s.borrow_mut().clear());
            let mut events: Vec<String> = Vec::new();
            // a new session (the first frame, then now and then)
            if session_frames == 0 || (next() % 3000 == 0 && new_sessions < 2) {
                new_sessions += 1;
                new_session(lambda.as_mut_ptr(), std::ptr::null());
                port.new_session(game_time, &mut host);
                events.push("new session".into());
            }
            // Drive (after a frame of the cars held)
            if port.status == DragState::Still && still_frames > 0 && !f.paused && !f.replay && next() % 40 == 0 {
                wr(d, DRAG_STATUS, 1u32);
                port.drive_pressed();
                speeds = [0.0; 2];
                react = [(next() % 60) as f32 * 0.01, (next() % 40) as f32 * 0.01];
                dq_line = [next() % 15 == 0, next() % 25 == 0];
                accel = [5.0 + (next() % 4) as f32, 5.0 + (next() % 4) as f32];
                jump = [next() % 6 == 0, next() % 8 == 0];
                events.push("drive".into());
            }
            // the made-up drive
            if !f.paused && !f.replay {
                let racing = port.status == DragState::RaceOn;
                for i in 0..2 {
                    let p = &mut f.positions[i];
                    if racing && jump[i] && game_time < port.start_time && game_time > port.start_time - 3000.0 {
                        jump[i] = false;
                        p[2] += 0.05 + (next() % 30) as f32 * 0.01; // a jump start
                    }
                    if racing && game_time >= port.start_time + react[i] as f64 * 1000.0 && port.cars[i].final_speed == 0 {
                        speeds[i] += accel[i] * dt;
                        p[2] += speeds[i] * dt;
                        if dq_line[i] {
                            p[0] += if i == 0 { 0.6 } else { -0.6 } * dt;
                        }
                    }
                }
                f.state_positions = f.positions.clone();
            }
            for (i, a) in avatars.iter().enumerate() {
                wr(*a, CA_BODY_POSITION, f.positions[i]);
                wr(*a, CA_STATE_POSITION, f.state_positions[i]);
            }
            still_frames = if port.status == DragState::Still { still_frames + 1 } else { 0 };
            update(d, dt);
            port.update(dt, &f, &mut host);
            // the finish line
            let mut port_said = Vec::new();
            for i in 0..2 {
                if f.positions[i][2] >= finish_nodes[0][2] && port.cars[i].final_speed == 0 && port.status == DragState::RaceOn && !f.paused && !f.replay {
                    wr(avatars[i], CA_DRAG_SPEED, speeds[i]);
                    let index = i as u32;
                    finish(d, &index);
                    port.on_finish_lane_passed(i, speeds[i], use_mph, &mut host, &mut |a, b| port_said.push(format!("Message({a} | {b})")));
                    events.push(format!("finish {i} at {}", speeds[i]));
                }
            }
            let game_log = DRAG.with(|w| std::mem::take(&mut w.borrow_mut().log));
            let game_said = SAID.with(|s| std::mem::take(&mut *s.borrow_mut()));
            for line in &game_log {
                if let Some(i) = line.strip_prefix("spawn ") {
                    let i: usize = i.parse().unwrap();
                    f.positions[i] = spawns[i];
                    f.state_positions[i] = spawns[i];
                }
            }
            let game_state = game_side_drag(d, semaphore, &avatars);
            let port_state = port_side_drag(&port);
            let (goals, tier) = {
                let g = game_side(d);
                (g.1, g.2)
            };
            if game_state != port_state || game_log != host.log || game_said != port_said || (&goals, tier) != (&port.base.conditions, port.base.current_tier) {
                return Err(format!(
                    "drag session {session} frame {session_frames} dt {dt} {events:?} paused {} replay {}:\n the game {game_state:?}\n {game_log:?} said {game_said:?} tier {tier}\n the port {port_state:?}\n {:?} said {port_said:?} tier {}",
                    f.paused, f.replay, host.log, port.base.current_tier
                ));
            }
            report.drag_frames += 1;
            report.drag_messages += game_said.len();
            session_frames += 1;
            done += 1;
            if port.status == DragState::GameOver {
                over_frames += 1;
            }
            if over_frames > 400 || session_frames > 6000 {
                break;
            }
        }
        report.drag_sessions += 1;
        report.drag_matches += port.runs() as usize;
        report.drag_penalties += port.cars.iter().map(|c| c.false_start as usize + c.disqualified as usize).sum::<usize>();
    }
    Ok(())
}
