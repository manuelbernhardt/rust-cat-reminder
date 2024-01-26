use std::collections::HashMap;
use super::protocol::Message;

use message_io::network::{Endpoint, NetEvent, Transport, ToRemoteAddr, SendStatus};
use message_io::node::{self, NodeEvent};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use chrono::{DateTime, Utc};
use crate::reminder::ReminderEvent;

enum Signal {
    Tick
}

pub enum TransportEvent {
    NodeListUpdated(HashMap<String, Vec<Ipv4Addr>>),
    CleaningTimeReset(DateTime<Utc>)
}

pub fn run(ip_addr: IpAddr, port: u16, reminder_tx: Sender<ReminderEvent>, rx: Receiver<TransportEvent>, initial_state: DateTime<Utc>, shutdown_flag: Arc<AtomicBool>) {
    let addr: SocketAddr = SocketAddr::new(ip_addr, port);

    let (handler, listener) = node::split();

    match handler.network().listen(Transport::Udp, addr) {
        Ok((_id, real_addr)) =>
            log::info!("Server running at {}", real_addr),
        Err(_) =>
            panic!("Can not listen at {}", addr)
    }

    let mut other_nodes_connections: HashMap<String, Endpoint> = HashMap::new();
    let mut last_modification_time: DateTime<Utc> = initial_state;

    handler.signals().send_with_timer(Signal::Tick, Duration::from_millis(500));

    std::thread::spawn(move || {
        listener.for_each(move |event| match event {
            NodeEvent::Network(net_event) => match net_event {
                NetEvent::Message(endpoint, input_data) => {
                    let message: Message = bincode::deserialize(&input_data).unwrap();
                    match message {
                        Message::RequestState => {
                            let reply = Message::UpdateState(Some(last_modification_time));
                            let output_data = bincode::serialize(&reply).unwrap();
                            handler.network().send(endpoint, &output_data);
                        }
                        Message::UpdateState(new_state) => {
                            log::info!("Update state received from network");
                            if let Some(timestamp) = new_state {
                                last_modification_time = timestamp;
                                reminder_tx.send(ReminderEvent::CleaningTimeUpdated(timestamp)).expect("Failed to send updated state")
                            }
                        }
                    }
                }
                _ => ()
            },
            NodeEvent::Signal(signal) => match signal {
                Signal::Tick => {
                    // see if there are updated nodes from mDNS
                    if let Ok(msg) = rx.try_recv() {
                        match msg {
                            TransportEvent::NodeListUpdated(list) => {
                                log::info!("Updating node list {:?}", list);
                                let new_node_connections: HashMap<String, Endpoint> = list.iter()
                                    .filter(|(k, _)| { !&other_nodes_connections.contains_key(k.as_str()) })
                                    .flat_map(|(k, ips)| {
                                        ips.iter().map(|ip| {
                                            let (receiver_id, _) =
                                                handler.network().connect_sync(Transport::Udp, format!("{}:{}", ip.clone().to_string(), port).to_remote_addr().expect("Failed to convert remote address")).expect("Failed to connect");
                                            (k.clone(), receiver_id)
                                        }).collect::<Vec<_>>()
                                }).collect();
                                let require_state = other_nodes_connections.len() == 0 && new_node_connections.len() > 0;
                                other_nodes_connections.extend(new_node_connections);
                                other_nodes_connections.retain(|k, _| {
                                    list.contains_key(k.as_str())
                                });
                                log::info!("Done updating connections: {:?}", other_nodes_connections);
                                if require_state {
                                    log::info!("Requesting state update from the network");
                                    if let Some((_, endpoint)) = &other_nodes_connections.iter().next() {
                                        let msg = Message::RequestState;
                                        let output_data = bincode::serialize(&msg).unwrap();
                                        let status = handler.network().send(**endpoint, &output_data);
                                        log::info!("Send status {:?}", status);
                                    }
                                }
                            }
                            TransportEvent::CleaningTimeReset(updated_time) => {
                                log::info!("Starting to send updated state");
                                last_modification_time = updated_time;
                                other_nodes_connections.iter().for_each(|(id, endpoint)| {
                                    log::info!("Sending updated state to {}", id);
                                    let msg = Message::UpdateState(Some(updated_time));
                                    let output_data = bincode::serialize(&msg).unwrap();
                                    let status: SendStatus = handler.network().send(*endpoint, &output_data);
                                    log::info!("Send status {:?}", status);
                                });
                            }
                        }
                    }

                    if shutdown_flag.load(Ordering::Relaxed) {
                        handler.stop();
                    } else {
                        handler.signals().send_with_timer(Signal::Tick, Duration::from_millis(500));
                    }
                }
            }
        });
    });

}
