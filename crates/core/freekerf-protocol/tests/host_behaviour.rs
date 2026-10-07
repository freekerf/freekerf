//! Behaviour of the host beyond the conformance protocol scenarios (the white-box
//! characterization of `GrblCore` in `conformance/tests/whitebox`).
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use freekerf_emulator::DeviceConfig;
use freekerf_gcode::Decimal;
use freekerf_protocol::firmware::FirmwareKind;
use freekerf_protocol::{
    AutoCooling, ConsoleRow, Event, Host, HostError, HostOptions, Issue, JogDirection, MachineStatus,
    MessageKind, Output, Point3, StreamingMode, SystemOpKind,
};
use freekerf_testkit::{Bench, job_lines};

fn connected() -> Bench {
    let mut b = Bench::new();
    b.connect();
    b
}

fn console_texts(b: &Bench) -> Vec<String> {
    b.events
        .iter()
        .filter_map(|e| match e {
            Event::Console(ConsoleRow::Message { text, .. }) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------- jobs

#[test]
fn comment_lines_are_streamed_as_empty_lines_and_header_footer_wrap_the_job() {
    let mut b = connected();
    b.load("(comment)\nG1 X1 F100\n");
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| !b.host.in_program(), 5000));
    assert_eq!(b.lines(), ["G90", "", "G1X1F100", "G0X0Y0Z0"]);
}

#[test]
fn run_program_preconditions() {
    let mut h = Host::for_kind(FirmwareKind::Grbl, HostOptions::default());
    assert_eq!(h.run_program(false, 0), Err(HostError::NotConnected));
    let mut b = Bench::new();
    b.open();
    // connect-time queries still to come
    b.advance(50);
    b.load("G1 X1");
    assert_eq!(b.host.run_program(false, b.now), Err(HostError::Busy));
    assert!(b.run_until(|b| b.host.ready(), 5000));
    b.host.set_program(freekerf_gcode::Program::new()).unwrap();
    assert_eq!(b.host.run_program(false, b.now), Err(HostError::NoProgram));
    b.load("G1 X1");
    b.device.alarm_now(1);
    b.exchange();
    b.advance(600);
    assert!(matches!(
        b.host.run_program(false, b.now),
        Err(HostError::NotAllowed(_))
    ));
}

#[test]
fn run_with_homing_pushes_dollar_h_first() {
    let mut b = connected();
    b.load("G1 X1 F100");
    b.host.run_program(true, b.now).unwrap();
    b.exchange();
    assert_eq!(b.lines()[0], "$H");
}

#[test]
fn loop_count_repeats_the_program_with_the_passes_code() {
    let mut b = connected();
    b.load("G1 X1 F6000");
    b.host.set_loop_count(3);
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| !b.host.in_program() && b.host.ready(), 10_000));
    let l = b.lines();
    assert_eq!(l.iter().filter(|x| *x == "G1X1F6000").count(), 3);
    // default passes code: four comment lines streamed as empty lines, twice
    assert_eq!(l.iter().filter(|x| x.is_empty()).count(), 8);
    assert_eq!(l.last().unwrap(), "G0X0Y0Z0");
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, Event::PassEnded { remaining: 1 }))
    );
    assert_eq!(
        b.events
            .iter()
            .filter(|e| matches!(e, Event::JobEnded { .. }))
            .count(),
        1
    );
}

#[test]
fn custom_header_and_footer_with_expressions_and_immediates() {
    let opts = HostOptions {
        custom_header: "M3 S[$30/2]\n?\n0x85\n!\n\nnot[valid".into(),
        custom_footer: "M5\nG0 X[left] Y[bottom]".into(),
        ..HostOptions::default()
    };
    let mut b = Bench::with(opts, DeviceConfig::default());
    b.connect();
    b.load("G0 X5 Y5\nM3 S10\nG1 X10 Y8 F6000\nM5");
    b.host.run_program(false, b.now).unwrap();
    b.exchange();
    let rt = b.device.realtime();
    assert!(rt.contains(&0x3F) && rt.contains(&0x85) && rt.contains(&0x21));
    b.host.cycle_start(b.now).ok();
    b.device.receive(b"~", b.now);
    assert!(b.run_until(|b| !b.host.in_program(), 10_000));
    let l = b.lines();
    assert_eq!(l[0], "M3S500.000");
    assert_eq!(l[1], "not[valid");
    assert_eq!(l[l.len() - 1], "G0X5.000Y5.000");
    assert_eq!(b.host.evaluate_expression("[width]x[height]"), "5.000x3.000");
}

#[test]
fn custom_code_sees_the_jog_step_and_speed() {
    // LaserGRBL's [jogstep] and [jogspeed] (conformance
    // test_jobs.py::test_custom_code_expressions_see_the_jog_step_and_speed)
    let opts = HostOptions {
        jog_step_mm: 2.5,
        jog_speed_mm_min: 1200.0,
        ..HostOptions::default()
    };
    let mut b = Bench::with(opts, DeviceConfig::default());
    b.connect();
    assert_eq!(
        b.host.evaluate_expression("X[jogstep*2] F[jogspeed]"),
        "X5.000 F1200.000"
    );
    b.host
        .execute_custom_code("G1 X[jogstep] F[jogspeed]", b.now)
        .unwrap();
    b.exchange();
    assert_eq!(b.lines().last().unwrap(), "G1X2.500F1200.000");
}

#[test]
fn errors_are_counted_and_decoded_in_the_console() {
    let mut b = connected();
    b.device.config_mut().error_rules = vec![(regex::Regex::new("X2").unwrap(), 2)];
    b.load("G1 X1 F100\nG1 X2\nG1 X3");
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| !b.host.in_program(), 5000));
    assert!(b.events.iter().any(|e| matches!(e,
        Event::Console(ConsoleRow::Result { result, decoded: Some(d), .. }) if result == "ERROR:2" && d == "Bad number format")));
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, Event::JobEnded { errors: 1, .. }))
    );
    assert!(
        console_texts(&b)
            .iter()
            .any(|t| t.starts_with("[3 lines, 1 errors"))
    );
}

#[test]
fn abort_without_job_is_refused() {
    let mut b = connected();
    assert!(matches!(
        b.host.abort_program(b.now),
        Err(HostError::NotAllowed(_))
    ));
}

#[test]
fn resume_from_position_rebuilds_state() {
    let mut b = connected();
    b.load("G90\nG0 X10 Y10\nM3 S500\nG1 X20 F800\nG91\nG1 Y5\nG1 X-3\nM5");
    b.host.resume_program(5, false, false, b.now).unwrap();
    assert!(b.run_until(|b| !b.host.in_program(), 5000));
    // no header on resume; the footer follows as after any job
    assert_eq!(
        b.lines(),
        [
            "G90",
            "M5G0X20Y10Z0F800S500",
            "G91M3",
            "G1Y5",
            "G1X-3",
            "M5",
            "G0X0Y0Z0"
        ]
    );
}

#[test]
fn resume_adds_the_motion_mode_homing_and_work_offset() {
    let mut b = connected();
    b.load("G1 X5 F600\nX6\nX7");
    // interrupted job: remember a work offset during the job
    b.host.run_program(false, b.now).unwrap();
    b.device.receive(b"G92X1\n", b.now);
    b.device.set_auto_ack(false);
    b.advance(600);
    b.host.close(false, b.now);
    let mut b2 = connected();
    b2.load("G1 X5 F600\nX6\nX7");
    b2.host.resume_program(1, true, false, b2.now).unwrap();
    assert!(b2.run_until(|b| !b.host.in_program(), 5000));
    assert_eq!(
        b2.lines(),
        ["$H", "G90", "M5G0X5Y0Z0F600S0", "G1X6", "X7", "G0X0Y0Z0"]
    );
    assert!(
        b.host.snapshot().last_issue == Issue::Unknown
            || b.host.snapshot().last_issue == Issue::UnexpectedDisconnect
    );
}

#[test]
fn resume_suggestion_after_an_interruption() {
    let mut b = connected();
    b.load(&job_lines(40).join("\n"));
    assert_eq!(b.host.resume_suggestion(), None);
    b.device.set_auto_ack(false);
    b.host.run_program(false, b.now).unwrap();
    b.exchange();
    b.device.release(5);
    assert!(b.run_until(|b| b.host.status() == MachineStatus::Run, 2000));
    b.host.soft_reset(b.now).unwrap();
    b.exchange();
    let s = b.host.resume_suggestion().unwrap();
    assert_eq!(s.executed, 5);
    assert_eq!(s.issue, Issue::ManualReset);
    assert!(!s.homing);
}

#[test]
fn unexpected_reset_during_a_job_is_reported() {
    let mut b = connected();
    b.device.set_auto_ack(false);
    b.load(&job_lines(30).join("\n"));
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| b.host.status() == MachineStatus::Run, 2000));
    b.device.announce();
    b.exchange();
    assert!(b.issues().contains(&Issue::UnexpectedReset));
    assert!(!b.host.in_program());
}

#[test]
fn disconnect_during_a_job_is_reported_unless_user() {
    for (user, issue) in [(false, Some(Issue::UnexpectedDisconnect)), (true, None)] {
        let mut b = connected();
        b.device.set_auto_ack(false);
        b.load(&job_lines(30).join("\n"));
        b.host.run_program(false, b.now).unwrap();
        assert!(b.run_until(|b| b.host.status() == MachineStatus::Run, 2000));
        b.host.close(user, b.now);
        b.exchange();
        assert_eq!(b.issues().first().copied(), issue);
        assert_eq!(b.host.status(), MachineStatus::Disconnected);
    }
}

#[test]
fn hang_is_detected_when_status_reports_stop() {
    let mut b = connected();
    b.device.set_auto_ack(false);
    b.load(&job_lines(30).join("\n"));
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| b.host.status() == MachineStatus::Run, 2000));
    b.open = false; // the board goes silent
    for _ in 0..600 {
        b.now += 10;
        b.host.tick(b.now);
        while b.host.poll_output().is_some() {}
        while let Some(e) = b.host.poll_event() {
            b.events.push(e);
        }
    }
    assert!(b.issues().contains(&Issue::StopResponding));
}

#[test]
fn buffer_stuck_is_unlocked_when_the_board_reports_an_empty_buffer() {
    let mut b = connected();
    b.load(&job_lines(30).join("\n"));
    b.device.set_auto_ack(false);
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| b.host.status() == MachineStatus::Run, 2000));
    assert!(!b.host.unlock_buffer_stuck(b.now));
    // the oks were lost: the board executed everything, its buffer is empty
    let n = b.device.pending_lines();
    b.device.release(n);
    b.device.take_output();
    b.now += 11_000;
    b.host
        .receive(b"<Run|MPos:0.000,0.000,0.000|Bf:15,128|FS:0,0>\n", b.now);
    b.exchange();
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, Event::BufferStuckUnlocked { .. }))
    );
}

#[test]
fn manual_buffer_stuck_unlock() {
    let mut b = Bench::with(
        HostOptions::default(),
        DeviceConfig {
            report_buffer: false,
            ..DeviceConfig::default()
        },
    );
    b.connect();
    b.load(&job_lines(30).join("\n"));
    b.device.set_auto_ack(false);
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| b.host.status() == MachineStatus::Run, 2000));
    b.now += 11_000;
    assert!(b.host.unlock_buffer_stuck(b.now));
}

#[test]
fn auto_cooling_holds_and_resumes() {
    let opts = HostOptions {
        auto_cooling: Some(AutoCooling {
            on_ms: 1000,
            off_ms: 500,
        }),
        ..HostOptions::default()
    };
    let mut b = Bench::with(
        opts,
        DeviceConfig {
            timed: true,
            ..DeviceConfig::default()
        },
    );
    b.connect();
    // 40 one-second moves: the job lasts longer than the 15-block planner
    let moves: Vec<String> = (0..40)
        .map(|i| format!("G1 X{} F600", if i % 2 == 0 { 10 } else { 0 }))
        .collect();
    b.load(&format!("M3 S100\n{}\nM5", moves.join("\n")));
    b.host.run_program(false, b.now).unwrap();
    assert!(b.run_until(|b| b.host.status() == MachineStatus::Cooling, 3000));
    assert!(b.run_until(|b| b.host.status() == MachineStatus::Run, 3000));
    let holds = b.device.realtime().iter().filter(|r| **r == b'!').count();
    let resumes = b.device.realtime().iter().filter(|r| **r == b'~').count();
    assert!(holds >= 1 && resumes >= 1);
}

#[test]
fn time_projection_follows_the_job() {
    let mut b = Bench::with(
        HostOptions::default(),
        DeviceConfig {
            timed: true,
            ..DeviceConfig::default()
        },
    );
    b.connect();
    let moves: Vec<String> = (0..30)
        .map(|i| format!("G1 X{} F600", if i % 2 == 0 { 10 } else { 0 }))
        .collect();
    b.load(&moves.join("\n"));
    b.host.run_program(false, b.now).unwrap();
    b.advance(500);
    let s = b.host.snapshot();
    assert!(s.in_program && s.job_target == 31 && s.program_lines == 30); // header + program
    assert!(s.projected_ms > 0);
    assert!(b.run_until(|b| !b.host.in_program(), 40_000));
    assert!(b.host.projection().total_job_ms(b.now) >= 14_000);
}

// ---------------------------------------------------------------- immediate commands and permissions

#[test]
fn state_rules_of_immediate_commands() {
    let mut b = connected();
    assert!(b.host.feed_hold(b.now).is_err());
    assert!(b.host.cycle_start(b.now).is_err());
    assert!(matches!(b.host.home(b.now), Err(HostError::NotAllowed(_)))); // $22=0
    assert!(b.host.set_new_zero(b.now).is_err()); // already at zero
    b.host.send_command("G0 X5", b.now).unwrap();
    b.exchange();
    b.advance(600);
    b.host.set_new_zero(b.now).unwrap();
    b.exchange();
    assert_eq!(b.lines().last().unwrap(), "G92X0Y0Z0");
    b.host.safety_door(b.now);
    b.exchange();
    assert!(b.device.realtime().contains(&0x84));
    let mut h = Host::for_kind(FirmwareKind::Grbl, HostOptions::default());
    assert_eq!(h.soft_reset(0), Err(HostError::NotConnected));
    assert_eq!(h.send_command("G0", 0), Err(HostError::NotConnected));
    assert_eq!(h.execute_custom_code("G0", 0), Err(HostError::NotConnected));
    assert!(h.unlock(0).is_err());
    assert!(h.write_settings(&[], 0).is_err());
    assert!(h.refresh_settings(0).is_err());
    assert!(h.write_wifi("a", "b", 0).is_err());
}

#[test]
fn manual_commands_are_refused_during_a_job() {
    let mut b = connected();
    b.device.set_auto_ack(false);
    b.load(&job_lines(30).join("\n"));
    b.host.run_program(false, b.now).unwrap();
    b.exchange();
    assert!(matches!(
        b.host.send_command("G0", b.now),
        Err(HostError::NotAllowed(_))
    ));
    assert!(b.host.set_program(freekerf_gcode::Program::new()).is_err());
    assert!(b.host.write_settings(&[(1, "1".into())], b.now).is_err());
}

#[test]
fn soft_reset_works_while_connecting() {
    let mut b = Bench::with(
        HostOptions {
            reset_on_connect: false,
            ..HostOptions::default()
        },
        DeviceConfig::default(),
    );
    b.open = true;
    b.host.open(b.now);
    b.host.soft_reset(b.now).unwrap();
    b.exchange();
    assert_eq!(b.device.resets(), 1);
}

// ---------------------------------------------------------------- RX parsing

#[test]
fn status_report_fields() {
    let mut b = connected();
    b.host.receive(
        b"<Idle|WPos:1.000,2.000,3.000|WCO:1,1,1|FS:300,20|Bf:10,100|Ov:110,50,90|A:S>\n",
        b.now,
    );
    let s = b.host.snapshot();
    assert_eq!(s.work_offset, Point3::new(1.0, 1.0, 1.0));
    assert_eq!(s.machine_position, Point3::new(1.0, 2.0, 3.0)); // WPos used the previous WCO (zero)
    assert_eq!((s.feed, s.power), (300.0, 20.0));
    assert_eq!((s.grbl_blocks, s.grbl_buffer), (10, 100));
    assert_eq!(s.overrides, [110, 50, 90]);
    b.host.receive(b"<Idle|F:500>\n", b.now);
    assert_eq!(b.host.snapshot().feed, 500.0);
    // unknown state: ignored
    b.host.receive(b"<Sleep|MPos:9,9,9>\n", b.now);
    assert_eq!(b.host.snapshot().machine_position, Point3::new(1.0, 2.0, 3.0));
}

#[test]
fn legacy_status_reports_without_version() {
    let opts = HostOptions {
        reset_on_connect: false,
        query_machine_info: false,
        ..HostOptions::default()
    };
    let mut h = Host::for_kind(FirmwareKind::Grbl, opts);
    h.open(0);
    h.receive(b"<Idle,MPos:5.000,6.000,7.000,WPos:1.000,2.000,3.000>\n", 10);
    let s = h.snapshot();
    assert_eq!(s.status, MachineStatus::Idle);
    assert_eq!(s.machine_position, Point3::new(5.0, 6.0, 7.0));
    assert_eq!(s.work_offset, Point3::new(4.0, 4.0, 4.0));
    // a `|` report with Pin: is read with the 1.1 parser (F-15 fixed)
    h.receive(b"<Idle|MPos:1,1,1|Pn:XY>\n", 20);
    assert_eq!(h.snapshot().machine_position, Point3::new(1.0, 1.0, 1.0));
}

#[test]
fn hold_states_depend_on_who_requested_them() {
    let mut b = connected();
    b.host.receive(b"<Hold:0|MPos:0,0,0>\n", b.now);
    assert_eq!(b.host.status(), MachineStatus::AutoHold);
    b.host.receive(b"<Run|MPos:0,0,0>\n", b.now);
    b.host.feed_hold(b.now).unwrap();
    b.host.receive(b"<Hold:0|MPos:0,0,0>\n", b.now);
    assert_eq!(b.host.status(), MachineStatus::Hold);
    b.host.receive(b"<Door:0|MPos:0,0,0>\n", b.now);
    b.host.cycle_start(b.now).unwrap();
}

#[test]
fn firmware_banners_and_vendors() {
    let cases: &[(&[&str], &str, Option<&str>, bool)] = &[
        (&["GrblHAL 1.1f ['$' or '$HELP' for help]"], "1.1f", None, true),
        (
            &["Grbl-Vigo:1.1f|Build:G-20170131-V3.0-20200720"],
            "1.1f",
            Some("Grbl-Vigo"),
            false,
        ),
        (
            &[
                "Ortur Laser Master 3 Ready!",
                "OLF 170.",
                "Grbl 1.1f ['$' for help]",
            ],
            "1.1f",
            Some("Ortur Laser Master 3"),
            false,
        ),
        (
            &["Aufero AL1 Ready!", "Grbl 1.1h"],
            "1.1h",
            Some("Aufero AL1"),
            false,
        ),
        (
            &["[Machine:Longer Nano]", "[Software:1.2]"],
            "1.1f",
            Some("Longer Nano"),
            false,
        ),
        (
            &["SimpleLaser 1.1f ['$' for help]"],
            "1.1f",
            Some("SimpleLaser"),
            false,
        ),
        (&["Fluid 3.4a (wifi)"], "3.4a", Some("Fluid"), false),
    ];
    for (lines, version, vendor, hal) in cases {
        let opts = HostOptions {
            reset_on_connect: false,
            ..HostOptions::default()
        };
        let mut h = Host::for_kind(FirmwareKind::Grbl, opts);
        h.open(0);
        for l in *lines {
            h.receive(format!("{l}\r\n").as_bytes(), 1);
        }
        let v = h.version().unwrap_or_else(|| panic!("{lines:?}"));
        assert_eq!(v.to_string(), *version, "{lines:?}");
        assert_eq!(v.vendor_info.as_deref(), *vendor, "{lines:?}");
        assert_eq!(v.hal, *hal);
    }
    let mut h = Host::for_kind(FirmwareKind::Grbl, HostOptions::default());
    h.open(0);
    h.receive(b"Grbl x.yz\n", 1); // malformed: no version, logged
    assert!(h.version().is_none());
    h.receive(b"Other 2.0x\n", 1); // unknown banner only before any banner
    assert!(h.version().is_none());
}

#[test]
fn ip_messages_alarms_and_broken_oks() {
    let mut b = connected();
    b.host.receive(
        b"[MSG:Get IP 192.168.1.182]\n[MSG:Connected with 10.0.0.2]\nALARM:1\nALARM:99\n",
        b.now,
    );
    assert_eq!(b.host.snapshot().detected_ip.as_deref(), Some("10.0.0.2"));
    while let Some(e) = b.host.poll_event() {
        b.events.push(e);
    }
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, Event::IpDetected(ip) if ip == "192.168.1.182"))
    );
    assert!(b.events.iter().any(|e| matches!(e,
        Event::Console(ConsoleRow::Message { kind: MessageKind::Alarm, decoded: Some(d), .. }) if d == "Hard limit")));
    // a garbled ok still acknowledges a line
    b.device.set_auto_ack(false);
    b.host.send_command("G0 X1", b.now).unwrap();
    b.exchange();
    assert_eq!(b.host.snapshot().pending, 1);
    b.host.receive(b"xxokxx\n", b.now);
    assert_eq!(b.host.snapshot().pending, 0);
    // a response without a pending command is ignored
    b.host.receive(b"ok\n", b.now);
}

#[test]
fn eeprom_writes_go_one_at_a_time_and_update_the_table() {
    let mut b = connected();
    b.device.set_auto_ack(false);
    let id = b
        .host
        .write_settings(
            &[(110, "1000".into()), (111, "1000".into()), (999, "1".into())],
            b.now,
        )
        .unwrap();
    b.exchange();
    assert_eq!(b.device.pending_lines(), 1);
    b.device.release(1);
    b.exchange();
    assert_eq!(b.device.pending_lines(), 1);
    b.device.set_auto_ack(true);
    b.exchange();
    assert_eq!(b.host.settings().max_rate_x(), Decimal::from(1000));
    assert!(b.events.iter().any(|e| matches!(e,
        Event::SystemOpFinished { id: i, kind: SystemOpKind::WriteSettings, result: Err(msg) } if *i == id && msg.contains("$999=1"))));
}

#[test]
fn settings_refresh_and_timeouts() {
    let mut b = connected();
    let id = b.host.refresh_settings(b.now).unwrap();
    b.exchange();
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, Event::SystemOpFinished { id: i, result: Ok(()), .. } if *i == id))
    );
    // a board that never answers: the query fails after 10 s, the late answer is absorbed
    b.device.set_auto_ack(false);
    let id = b.host.refresh_settings(b.now).unwrap();
    b.exchange();
    assert!(!b.host.ready());
    b.advance(10_100);
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, Event::SystemOpFinished { id: i, result: Err(_), .. } if *i == id))
    );
    b.device.set_auto_ack(true);
    b.exchange();
    assert_eq!(b.host.snapshot().pending, 0);
}

#[test]
fn settings_read_when_ok_comes_first() {
    let mut b = Bench::with(
        HostOptions::default(),
        DeviceConfig {
            ok_first: true,
            ..DeviceConfig::default()
        },
    );
    b.connect();
    assert_eq!(b.host.settings().len(), 34);
    assert_eq!(b.host.buffer_size(), 128);
}

#[test]
fn settings_query_rejected_keeps_the_old_table() {
    let mut b = connected();
    b.device.config_mut().error_rules = vec![(regex::Regex::new(r"^\$\$$").unwrap(), 3)];
    let id = b.host.refresh_settings(b.now).unwrap();
    b.exchange();
    b.advance(20);
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, Event::SystemOpFinished { id: i, result: Err(_), .. } if *i == id))
    );
    assert_eq!(b.host.settings().len(), 34);
}

#[test]
fn wifi_configuration_by_vendor() {
    let mut b = connected();
    assert!(matches!(
        b.host.write_wifi("net", "pw", b.now),
        Err(HostError::Unsupported(_))
    ));
    b.host
        .receive(b"Ortur Laser Master 3 Ready!\nGrbl 1.1f ['$' for help]\n", b.now);
    b.exchange();
    b.device.config_mut().settings.insert(74, String::new());
    b.device.config_mut().settings.insert(75, String::new());
    b.advance(600);
    assert!(b.host.write_wifi("net", "pw", b.now).unwrap().is_some());
    b.exchange();
    assert!(
        b.lines()
            .ends_with(&["$74=net".to_owned(), "$75=pw".to_owned(), "$WRS".to_owned()])
    );
    b.host.receive(b"[Machine:Longer Nano]\n", b.now);
    assert!(b.host.write_wifi("net", "pw", b.now).unwrap().is_none());
    b.exchange();
    assert!(b.lines().ends_with(&["$wifi/begin".to_owned()]));
}

// ---------------------------------------------------------------- jog

#[test]
fn jog_variants_v11() {
    let mut b = connected();
    b.host
        .jog(JogDirection::Home, 1000.0, Decimal::ONE, b.now)
        .unwrap();
    b.host
        .jog(JogDirection::Zdown, 100.0, Decimal::new(25, 2), b.now)
        .unwrap();
    b.host.jog_to(10.0, 20.5, 600.0, b.now).unwrap();
    b.exchange();
    assert!(b.lines().ends_with(&[
        "$J=G90X0Y0F1000".to_owned(),
        "$J=G91Z-0.25F100".to_owned(),
        "$J=G90X10.00Y20.50F600".to_owned()
    ]));
}

#[test]
fn jog_to_is_clamped_by_soft_limits() {
    let mut b = connected();
    b.host.write_settings(&[(20, "1".into())], b.now).unwrap();
    b.exchange();
    b.host.jog_to(1000.0, -5.0, 600.0, b.now).unwrap();
    b.exchange();
    assert_eq!(b.lines().last().unwrap(), "$J=G90X400.00Y0.00F600");
}

#[test]
fn jog_on_grbl_09_uses_plain_gcode() {
    let opts = HostOptions {
        reset_on_connect: false,
        query_machine_info: false,
        ..HostOptions::default()
    };
    let mut b = Bench::with(
        opts,
        DeviceConfig {
            version: "0.9j".into(),
            ..DeviceConfig::default()
        },
    );
    b.connect();
    b.host.jog(JogDirection::SW, 500.0, Decimal::ONE, b.now).unwrap();
    b.host
        .jog(JogDirection::Home, 500.0, Decimal::ONE, b.now)
        .unwrap();
    b.host.jog_to(1.0, 2.0, 500.0, b.now).unwrap();
    b.exchange();
    assert!(b.lines().ends_with(&[
        "G91".into(),
        "G1X-1.0Y-1.0F500".into(),
        "G90".into(),
        "G90".into(),
        "G1X0Y0F500".into(),
        "G90".into(),
        "G1X1.00Y2.00F500".into()
    ]));
    b.host.jog_abort(b.now); // not supported: nothing happens
}

#[test]
fn continuous_jog_sends_targets_and_aborts() {
    let opts = HostOptions {
        continuous_jog: true,
        ..HostOptions::default()
    };
    let mut b = Bench::with(opts, DeviceConfig::default());
    b.connect();
    b.host.jog(JogDirection::NE, 1000.0, Decimal::ONE, b.now).unwrap();
    b.exchange();
    assert_eq!(b.lines().last().unwrap(), "$J=G53X400.0Y300.0F1000");
    b.host.jog(JogDirection::W, 1000.0, Decimal::ONE, b.now).unwrap();
    b.exchange();
    assert_eq!(b.device.realtime().iter().filter(|r| **r == 0x85).count(), 1);
    assert_eq!(b.lines().last().unwrap(), "$J=G53X0.0F1000");
    b.host.jog_abort(b.now);
    b.exchange();
    assert_eq!(b.device.realtime().iter().filter(|r| **r == 0x85).count(), 2);
    b.host.jog_abort(b.now);
    b.exchange();
    assert_eq!(b.device.realtime().iter().filter(|r| **r == 0x85).count(), 2);
    b.host.jog_to(5.0, 5.0, 300.0, b.now).unwrap();
    b.host
        .jog(JogDirection::Home, 300.0, Decimal::ONE, b.now)
        .unwrap();
    b.host.jog(JogDirection::Zup, 300.0, Decimal::ONE, b.now).unwrap();
    b.exchange();
    assert!(b.lines().contains(&"$J=G91Z1.0F300".to_owned()));
    assert!(b.lines().last().unwrap().starts_with("$J=G90X0Y0F300"));
}

#[test]
fn jog_is_refused_when_not_idle() {
    let mut h = Host::for_kind(FirmwareKind::Grbl, HostOptions::default());
    assert!(!h.jog_enabled());
    assert!(h.jog(JogDirection::N, 1.0, Decimal::ONE, 0).is_err());
    assert!(h.jog_to(1.0, 1.0, 1.0, 0).is_err());
}

// ---------------------------------------------------------------- variants

fn variant(kind: FirmwareKind, device: DeviceConfig) -> Bench {
    let mut b = Bench::with_host(Host::for_kind(kind, HostOptions::default()), device);
    b.open();
    b
}

#[test]
fn smoothie_skips_soft_reset_sends_newline_and_streams_synchronously() {
    let mut b = variant(FirmwareKind::Smoothie, DeviceConfig::default());
    assert!(!b.device.realtime().contains(&0x18));
    assert!(b.run_until(|b| b.host.ready(), 5000));
    assert_eq!(b.host.firmware(), FirmwareKind::Smoothie);
    b.device.clear_lines();
    b.device.set_auto_ack(false);
    b.load(&job_lines(5).join("\n"));
    b.host.run_program(false, b.now).unwrap();
    b.exchange();
    assert_eq!(b.device.pending_lines(), 1);
    b.device.set_auto_ack(true);
    assert!(b.run_until(|b| !b.host.in_program(), 5000));
    b.host.receive(b"<Idle|F:100.5,20>\n", b.now);
    assert_eq!((b.host.snapshot().feed, b.host.snapshot().power), (100.5, 20.0));
    assert!(matches!(b.host.unlock(b.now), Err(HostError::Unsupported(_))));
    b.device.take_output();
    b.host.soft_reset(b.now).unwrap();
    let mut wrote = Vec::new();
    while let Some(Output::Write(w)) = b.host.poll_output() {
        wrote.extend(w);
    }
    assert_eq!(wrote, b"reset\r\n");
    assert!(b.host.jog(JogDirection::N, 1.0, Decimal::ONE, b.now).is_ok());
}

#[test]
fn marlin_polls_with_m114_and_reports_idle_at_the_end() {
    let mut h = Host::for_kind(FirmwareKind::Marlin, HostOptions::default());
    h.open(0);
    let first: Vec<Output> = std::iter::from_fn(|| h.poll_output()).collect();
    assert_eq!(first, vec![Output::Write(b"M114\n".to_vec())]);
    h.receive(b"X:10.00 Y:2.00 Z:0.00 E:0.00 Count X:1600 Y:0 Z:0\n", 10);
    assert_eq!(h.status(), MachineStatus::Idle);
    assert_eq!(h.snapshot().machine_position, Point3::new(10.0, 2.0, 0.0));
    assert!(h.ready()); // no $$/$I on Marlin
    h.tick(600);
    assert!(h.refresh_settings(600).is_err());
    h.set_program(freekerf_gcode::Program::parse(
        "G1 X1",
        &freekerf_gcode::AnalysisOptions::default(),
    ))
    .unwrap();
    h.run_program(false, 700).unwrap();
    while h.poll_output().is_some() {}
    h.receive(b"ok\nok\nok\n", 800);
    while h.poll_output().is_some() {}
    h.receive(b"ok\n", 900);
    assert!(!h.in_program());
    assert_eq!(h.status(), MachineStatus::Idle);
    h.receive(b"X:bad\n", 1000); // malformed: status only
    while h.poll_output().is_some() {}
    h.soft_reset(1000).unwrap();
    assert!(h.poll_output().is_none()); // Marlin has no soft reset
}

#[test]
fn vigo_wraps_jobs_and_uses_sbuf_counters() {
    let mut h = Host::for_kind(
        FirmwareKind::VigoWork,
        HostOptions {
            reset_on_connect: false,
            query_machine_info: false,
            ..HostOptions::default()
        },
    );
    h.open(0);
    let first: Vec<Output> = std::iter::from_fn(|| h.poll_output()).collect();
    assert_eq!(first, vec![Output::Write(vec![0x88])]);
    h.receive(
        b"Grbl-Vigo:1.1f|Build:G-20170131-V3.0-20200720\n<Idle|MPos:0,0,0>\n",
        10,
    );
    h.tick(600);
    while h.poll_output().is_some() {}
    h.receive(b"$0=1\nok\n", 620);
    h.tick(1200);
    assert!(h.ready(), "{:?}", h.snapshot());
    assert_eq!(h.buffer_size(), 127);
    h.set_program(freekerf_gcode::Program::parse(
        "G1 X1\nG1 X2",
        &freekerf_gcode::AnalysisOptions::default(),
    ))
    .unwrap();
    h.run_program(false, 1300).unwrap();
    let sent: Vec<String> = std::iter::from_fn(|| h.poll_output())
        .filter_map(|o| match o {
            Output::Write(w) => Some(String::from_utf8_lossy(&w).into_owned()),
            Output::Close => None,
        })
        .collect();
    let all = sent.concat();
    let magic = all.find(">Ofk9gsd8IKjKBahP0OGS9BrhZCPeWCBALCbyGf\n").unwrap();
    assert!(magic < all.find("G90\n").unwrap());
    h.receive(b"<VSta:2|SBuf:4,3,1|LTC:0>\n", 1400);
    assert_eq!(h.projection().errors(), 1);
    h.receive(b"<VSta:2|SBuf:5,4,1>\n", 1500);
    assert!(!h.in_program());
    let tail: Vec<String> = std::iter::from_fn(|| h.poll_output())
        .filter_map(|o| match o {
            Output::Write(w) => Some(String::from_utf8_lossy(&w).into_owned()),
            Output::Close => None,
        })
        .collect();
    assert!(tail.concat().contains("G0X0Y0M5\n"));
    h.receive(b"<VSta:0|SBuf:1,0,0>\n<VSta:x>\n<Idle|MPos:0,0,0>\n", 1600);
}

// ---------------------------------------------------------------- connection lifecycle

#[test]
fn connection_timeout_closes_the_port() {
    let mut h = Host::for_kind(FirmwareKind::Grbl, HostOptions::default());
    h.open(0);
    while h.poll_output().is_some() {}
    h.tick(10_001);
    assert!(std::iter::from_fn(|| h.poll_output()).any(|o| o == Output::Close));
    h.close(false, 10_002);
    assert_eq!(h.status(), MachineStatus::Disconnected);
    assert_eq!(h.snapshot().failed_connections, 1);
    h.open_failed(20_000);
    assert_eq!(h.snapshot().failed_connections, 2);
}

#[test]
fn next_deadline_follows_the_timers() {
    let mut h = Host::for_kind(FirmwareKind::Grbl, HostOptions::default());
    assert_eq!(h.next_deadline(), None);
    h.open(0);
    assert_eq!(h.next_deadline(), Some(500));
    let mut b = connected();
    b.load(&job_lines(5).join("\n"));
    b.device.set_auto_ack(false);
    b.host.run_program(false, b.now).unwrap();
    assert!(b.host.next_deadline().unwrap() <= b.now + 500);
    b.host.set_options(HostOptions {
        status_query_ms: 250,
        ..b.host.options().clone()
    });
    assert_eq!(b.host.options().status_query_ms, 250);
}

#[test]
fn laser_life_counters_follow_the_run_state() {
    let mut life = freekerf_protocol::LaserLife::default();
    life.counters
        .push(freekerf_protocol::LaserLifeCounter::new("a", "Default"));
    let mut b = Bench::with(
        HostOptions::default(),
        DeviceConfig {
            timed: true,
            ..DeviceConfig::default()
        },
    );
    b.host.set_laser_life(life);
    b.connect();
    assert!(b.host.select_laser("a"));
    b.load("M3 S1000\nG1 X50 F3000\nM5");
    b.host.run_program(false, b.now).unwrap();
    // the board acknowledges lines into its planner: it still runs after the job ended
    assert!(b.run_until(|b| !b.host.in_program(), 5000));
    b.advance(2000);
    assert!(b.host.laser_life().current().unwrap().run_ms > 0);
}

#[test]
fn several_laser_modules_ask_the_user() {
    let mut life = freekerf_protocol::LaserLife::default();
    life.counters
        .push(freekerf_protocol::LaserLifeCounter::new("a", "A"));
    life.counters
        .push(freekerf_protocol::LaserLifeCounter::new("b", "B"));
    let mut b = Bench::new();
    b.host.set_laser_life(life);
    b.open();
    assert!(b.events.iter().any(|e| matches!(e, Event::LaserSelectionNeeded)));
}

#[test]
fn streaming_mode_names() {
    for m in ["Buffered", "Synchronous", "RepeatOnError"] {
        assert!(StreamingMode::parse(m).is_some());
    }
    assert_eq!(StreamingMode::parse("Fast"), None);
}

#[test]
fn work_position_variables_are_the_work_position() {
    // DIV-101: LaserGRBL gives the machine position for [WPos.*]
    let mut b = connected();
    b.host.send_command("G0 X5", b.now).unwrap();
    b.host.send_command("G92 X0", b.now).unwrap();
    b.exchange();
    b.advance(600);
    assert_eq!(
        b.host.evaluate_expression("[WPos.X] [MPos.X] [WCO.X]"),
        "0.000 5.000 5.000"
    );
}
