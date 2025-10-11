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

    // new
    // let mut frame_accumulator = Vec::with_capacity(16000); // 预分配稍大空间
    // const FRAME_SIZE: usize = 8000;

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buffer) => {
                match result {
                    Ok((len, _src_addr)) => {
                        if tx.send(buffer[..len].to_vec()).await.is_err() {
                            log::warn!("主线程数据通道已关闭，UDP数据被丢弃。");
                        }
                        // 将收到的数据追加到累加器
                        // frame_accumulator.extend_from_slice(&buffer[..len]);
                        //
                        // // 检查累加器中是否已包含一个或多个完整的数据帧
                        // while frame_accumulator.len() >= FRAME_SIZE {
                        //     // 从累加器头部取出一个完整帧
                        //     let frame_data = frame_accumulator.drain(..FRAME_SIZE).collect::<Vec<u8>>();
                        //
                        //     // 发送这个完整帧到主线程
                        //     if tx.send(frame_data).await.is_err() {
                        //         log::warn!("主线程数据通道已关闭，UDP监听任务结束。");
                        //         return Ok(()); // 通道关闭，任务结束
                        //     }
                        // }
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