use privsep_rpn::rpn::handle_client;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    #[cfg(target_os = "openbsd")]
    pledge::pledge_promises![Stdio Rpath Wpath Cpath Inet].unwrap();

    let listener = TcpListener::bind("0.0.0.0:4000").await?;
    println!("RPN server listening on port 4000");

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream).await {
                eprintln!("client error: {e}");
            }
        });
    }
}
