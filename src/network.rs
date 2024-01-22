use std::collections::HashMap;
use super::protocol::Message;

use message_io::network::{Endpoint, NetEvent, Transport, ToRemoteAddr, SendStatus};
use message_io::node::{self, NodeEvent};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use chrono::{DateTime, Utc};
use crate::reminder::ReminderModuleEvent;

enum Signal {
    Tick
}

pub enum NetworkEvent {
    NodeListUpdated(HashMap<String, Vec<Ipv4Addr>>),
    StateUpdated(DateTime<Utc>)
}

pub fn run(ip_addr: IpAddr, port: u16, reminder_tx: Sender<ReminderModuleEvent>, rx: Receiver<NetworkEvent>, initial_state: DateTime<Utc>) {
    let addr: SocketAddr = SocketAddr::new(ip_addr, port);

    let (handler, listener) = node::split();

    match handler.network().listen(Transport::Udp, addr) {
        Ok((_id, real_addr)) =>
            println!("Server running at {}", real_addr),
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
                                reminder_tx.send(ReminderModuleEvent::CleaningTimeUpdate(timestamp)).expect("Failed to send updated state")
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
                            NetworkEvent::NodeListUpdated(list) => {
                                other_nodes_connections = list.iter().flat_map(|(k, ips)| {
                                    ips.iter().map(|ip| {
                                        let (receiver_id, _) =
                                            handler.network().connect_sync(Transport::Udp, format!("{}:{}", ip.clone().to_string(), port).to_remote_addr().unwrap()).unwrap();
                                        (k.clone(), receiver_id)
                                    }).collect::<Vec<_>>()
                                }).collect();
                            }
                            NetworkEvent::StateUpdated(updated_time) => {
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

                    handler.signals().send_with_timer(Signal::Tick, Duration::from_millis(500));
                }
            }
        });
    });

}
