use std::{
    io::{Read, Write},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
fn worker_rejects_oversized_protocol_header_and_exits() -> std::io::Result<()> {
    let (status, output) = run_worker(&u32::MAX.to_le_bytes())?;
    assert!(!status.success());
    assert!(output.is_empty());
    Ok(())
}

#[test]
fn worker_returns_structured_error_without_opening_a_camera() -> std::io::Result<()> {
    let request = br#""Read""#;
    let input = [(request.len() as u32).to_le_bytes().as_slice(), request].concat();
    let (status, output) = run_worker(&input)?;
    assert!(status.success());
    let expected = br#"{"Failed":"CameraNotOpen"}"#;
    assert_eq!(
        output,
        [(expected.len() as u32).to_le_bytes().as_slice(), expected].concat()
    );
    Ok(())
}

fn run_worker(input: &[u8]) -> std::io::Result<(ExitStatus, Vec<u8>)> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_camera_worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input)?;
    }
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(5) {
            child.kill()?;
            child.wait()?;
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let mut output = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_end(&mut output)?;
    }
    Ok((status, output))
}
