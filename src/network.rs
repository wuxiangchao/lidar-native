use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::Sender;
use tokio::sync::watch;

const MAX_DATAGRAM_SIZE: usize = 65_507;

// 一个用于发送十六进制指令的函数
pub async fn send_command(target_ip: &str, target_port: &str, command_hex: &str) -> anyhow::Result<()> {
    // 绑定到一个临时的本地端口用于发送
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let target_addr: SocketAddr = format!("{}:{}", target_ip, target_port).parse()?;

    // 将十六进制字符串转换为字节
    let bytes_to_send = hex::decode(command_hex.replace(" ", "").replace("0x", ""))?;

    socket.send_to(&bytes_to_send, &target_addr).await?;
    log::info!("Successfully sent command '{}' to {}", command_hex, target_addr);
    Ok(())
}


pub async fn run_udp_listener(
    ip: String,
    port: String,
    tx: Sender<Vec<u8>>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let local_addr: SocketAddr = format!("{}:{}", ip, port).parse()?;
    log::info!("开始监听UDP数据于: {}", local_addr);

    let socket = UdpSocket::bind(local_addr).await?;
    let mut buffer = vec![0u8; MAX_DATAGRAM_SIZE];

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buffer) => {
                match result {
                    Ok((len, _src_addr)) => {
                        if tx.send(buffer[..len].to_vec()).await.is_err() {
                            log::warn!("主线程数据通道已关闭，UDP数据被丢弃。");
                        }
                    }
                    Err(e) => {
                        log::error!("UDP接收错误: {}", e);
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("收到停止信号，UDP监听任务结束。");
                    break;
                }
            }
        }
    }
    Ok(())
}