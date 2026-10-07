use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, PtyPair, PtySize, native_pty_system};
use std::ffi::OsStr;
use std::io::{ErrorKind, Read, Write};

/// Prepares the shell command without starting a process.
pub fn build_shell_command(shell_path: &OsStr, login: bool) -> CommandBuilder {
    let mut command = CommandBuilder::new(shell_path);
    // Interactive mode lets the shell show a prompt and accept typed commands.
    command.arg("-i");

    if login {
        // Login mode loads startup files such as zsh's .zprofile.
        command.arg("-l")
    }

    command
}

/// Opens terminal pair that Stray and shell will use.
pub fn open_pty(size: PtySize) -> Result<PtyPair> {
    native_pty_system()
        .openpty(size)
        .context("could not open PTY")
}

/// Starts the shell on the shell's side of the PTY.
pub fn spawn_shell(pty: &PtyPair, command: CommandBuilder) -> Result<Box<dyn Child + Send + Sync>> {
    pty.slave
        .spawn_command(command)
        .context("could not start shell")
}

/// Copies shell output to the terminal until the stream ends.
pub fn forward_output(mut reader: impl Read, mut writer: impl Write) -> Result<()> {
    let mut buffer = [0u8; 8192];

    loop {
        let count = match reader.read(&mut buffer) {
            Ok(count) => count,
            // A signal can interrupt read, try again.
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("could not read shell output"),
        };

        if count == 0 {
            return Ok(());
        }

        writer
            .write_all(&buffer[..count])
            .context("could not write shell output")?;

        // Show prompts right away, even when no newline is present.
        writer.flush().context("could not flush shell output")?;
    }
}

/// Copies keyboard input to the shell until the input stream ends.
pub fn forward_input(mut reader: impl Read, mut writer: impl Write) -> Result<()> {
    std::io::copy(&mut reader, &mut writer).context("could not forward keyboard input")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::{self, BufWriter};

    #[test]
    fn input_preserves_text_and_control_keys() {
        // Include Enter, Tab, Backspace, Ctrl+C, and an arrow key.
        let input = b"echo hello\r\t\x7f\x03\x1b[A";
        let mut output = Vec::new();

        forward_input(&input[..], &mut output).expect("keyboard input should copy");

        assert_eq!(output, input);
    }

    #[test]
    fn input_reports_a_write_failure() {
        let mut output = [0u8; 2];

        let error = forward_input(&b"echo hello\r"[..], &mut output[..])
            .expect_err("writing past the available space should fail");

        assert_eq!(error.to_string(), "could not forward keyboard input");
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            ErrorKind::WriteZero
        );
    }

    #[test]
    fn shell_starts_and_returns_its_exit_code() {
        let pty = open_pty(PtySize::default()).expect("PTY should open");
        let mut command = CommandBuilder::new("/bin/sh");
        // Exit immediately so the test never waits for keyboard input.
        command.args(["-c", "exit 7"]);

        let mut child = spawn_shell(&pty, command).expect("shell should start");
        drop(pty.slave);
        let status = child.wait().expect("shell should finish");

        assert_eq!(status.exit_code(), 7);
    }

    #[test]
    fn missing_shell_returns_an_error() {
        let pty = open_pty(PtySize::default()).expect("PTY should open");
        // A regular file cannot contain another executable path.
        let command = CommandBuilder::new("/dev/null/stray-shell");

        let error = spawn_shell(&pty, command).expect_err("missing shell should fail");

        assert_eq!(error.to_string(), "could not start shell");
    }

    #[test]
    fn output_preserves_bytes_and_flushes_the_final_prompt() {
        // Use every byte value and enough data to require several reads.
        let mut input: Vec<u8> = (0..=255).cycle().take(20_000).collect();
        input.extend_from_slice(b"\x1b[32mshell> ");
        let mut output = Vec::new();
        let mut writer = BufWriter::with_capacity(1024, &mut output);

        forward_output(input.as_slice(), &mut writer).expect("output should copy");

        assert!(writer.buffer().is_empty(), "the prompt should be flushed");
        drop(writer);
        assert_eq!(output, input);
    }

    #[test]
    fn output_retries_an_interrupted_read() {
        struct InterruptedOnce {
            interrupted: bool,
            remaining: &'static [u8],
        }

        impl Read for InterruptedOnce {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(ErrorKind::Interrupted.into());
                }
                self.remaining.read(buffer)
            }
        }

        let reader = InterruptedOnce {
            interrupted: false,
            remaining: b"shell> ",
        };
        let mut output = Vec::new();

        forward_output(reader, &mut output).expect("interrupted read should retry");

        assert_eq!(output, b"shell> ");
    }

    #[test]
    fn output_reports_a_write_failure() {
        // A fixed slice runs out of space instead of growing like a Vec.
        let mut output = [0u8; 2];

        let error = forward_output(&b"shell> "[..], &mut output[..])
            .expect_err("writing past the available space should fail");

        assert_eq!(error.to_string(), "could not write shell output");
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            ErrorKind::WriteZero
        );
    }

    #[test]
    fn pty_uses_requested_size() {
        for (rows, cols) in [(24, 80), (40, 120), (10, 30)] {
            let size = PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            };

            // Read the size from a real PTY to check what the OS received.
            let pty = open_pty(size).expect("PTY should open");
            let actual_size = pty.master.get_size().expect("PTY size should be readable");

            assert_eq!(actual_size.rows, rows);
            assert_eq!(actual_size.cols, cols);
            assert_eq!(actual_size.pixel_width, 0);
            assert_eq!(actual_size.pixel_height, 0);
        }
    }

    #[test]
    fn shell_command_is_interactive_without_login() {
        for shell_path in ["/bin/zsh", "/bin/bash"] {
            let command = build_shell_command(OsStr::new(shell_path), false);
            let expected = vec![OsString::from(shell_path), OsString::from("-i")];

            assert_eq!(command.get_argv(), &expected);
        }
    }

    #[test]
    fn login_shell_command_is_interactive_and_has_login_flag() {
        for shell_path in ["/bin/zsh", "/bin/bash"] {
            let command = build_shell_command(OsStr::new(shell_path), true);
            let expected = vec![
                OsString::from(shell_path),
                OsString::from("-i"),
                OsString::from("-l"),
            ];

            assert_eq!(command.get_argv(), &expected);
        }
    }
}
