// Standalone finite ADB fixture. No sockets or real devices are accessed.
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};
fn main() {
    let root: PathBuf = std::env::current_exe().unwrap().parent().unwrap().into();
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("-H") {
        args.drain(..4);
    }
    let mode = fs::read_to_string(root.join("mode")).unwrap_or_default();
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("calls"))
        .unwrap();
    writeln!(log, "{}", args.join(" ")).unwrap();
    match args.first().map(String::as_str) {
        Some("version") => println!(
            "Android Debug Bridge version 1.0.{}",
            fs::read_to_string(root.join("protocol")).unwrap()
        ),
        Some("mdns") => {
            if mode == "mdns-disabled" {
                if args.get(1).is_some_and(|value| value == "check") {
                    println!("ERROR: mdns daemon unavailable");
                } else {
                    println!("List of discovered mdns services");
                }
                return;
            }
            if mode == "mdns-unavailable" {
                println!("unknown command: mdns");
                return;
            }
            println!("List of discovered mdns services");
            println!("adb-fixture-guid _adb-tls-pairing._tcp 192.0.2.10:37123");
            // The distractor shares an IP with the target but has a different GUID/port.
            println!("adb-other-guid _adb-tls-connect._tcp 192.0.2.10:49999");
            let name = if mode == "renamed" {
                "adb-fixture-guid (2)"
            } else {
                "adb-fixture-guid"
            };
            println!("{name} _adb-tls-connect._tcp 192.0.2.10:40123");
            if let Ok(name) = fs::read_to_string(root.join("qr-name")) {
                println!("{} _adb-tls-pairing._tcp 192.0.2.10:37123", name.trim());
            }
        }
        Some("pair") => {
            let mut code = String::new();
            std::io::stdin().read_to_string(&mut code).unwrap();
            fs::write(root.join("stdin-size"), code.len().to_string()).unwrap();
            fs::write(root.join("pair-started"), b"yes").unwrap();
            if mode == "slow" {
                std::thread::sleep(std::time::Duration::from_secs(8));
            }
            if code.trim() == "000000" {
                eprintln!("Fixture intentionally echoes secret: {code}");
                println!("Failed: wrong code {code}");
                return;
            }
            fs::write(root.join("paired"), b"yes").unwrap();
            println!("Successfully paired to 192.0.2.10:37123 [guid=adb-fixture-guid]");
        }
        Some("connect") => {
            assert_eq!(args[1], "192.0.2.10:40123", "wrong device selected");
            fs::write(root.join("connected"), b"yes").unwrap();
            println!("connected to {}", args[1]);
        }
        Some("devices") => {
            if mode == "disconnect" {
                eprintln!("fixture server disconnected");
                std::process::exit(1);
            }
            println!("List of devices attached");
            println!(
                "adb-other-guid._adb-tls-connect._tcp device model:Other_phone transport_id:9"
            );
            if root.join("connected").exists() || (mode == "auto" && root.join("paired").exists()) {
                let serial = if mode == "ip" || mode == "renamed" {
                    "192.0.2.10:40123"
                } else {
                    "adb-fixture-guid._adb-tls-connect._tcp"
                };
                println!("{serial} device model:Target_phone transport_id:5");
            }
        }
        Some("disconnect") => {
            assert!(matches!(
                args[1].as_str(),
                "192.0.2.10:40123" | "adb-fixture-guid._adb-tls-connect._tcp"
            ));
            let _ = fs::remove_file(root.join("connected"));
            fs::write(root.join("disconnected"), b"yes").unwrap();
            println!("disconnected {}", args[1]);
        }
        _ => panic!("unexpected command: {args:?}"),
    }
}
