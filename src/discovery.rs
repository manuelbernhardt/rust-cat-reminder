use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use rnglib::{Language, RNG};
use gethostname::gethostname;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use super::transport::TransportEvent;

const SERVICE_TYPE: &str = "_cat._udp.local.";


pub fn run(ip_addr: IpAddr, port: u16, network_tx: Sender<TransportEvent>, shutdown_flag: Arc<AtomicBool>) {
    let mdns = ServiceDaemon::new().expect("Failed to create mDNS daemon");

    let rng = RNG::try_from(&Language::Demonic).unwrap();
    let instance_name = rng.generate_name();
    log::info!("Instance name: {}", instance_name);
    let hostname = gethostname();
    let host_name: &str = std::str::from_utf8(hostname.as_bytes()).unwrap();
    let host_name_full = format!("{}.local.", host_name);
    log::info!("Hostname: {}", host_name_full);

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        instance_name.as_str(),
        host_name_full.as_str(),
        ip_addr,
        port,
        None
    ).unwrap().enable_addr_auto();

    let service_fullname = service_info.get_fullname().to_string();
    mdns.register(service_info).expect("Failed to register mDNS service");

    let mut cat_reminder_instances = HashMap::new();

    let receiver = mdns.browse(SERVICE_TYPE).expect("Failed to browse mDNS services");
    std::thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    let is_other_service = !info.get_fullname().starts_with(instance_name.as_str());
                    if is_other_service {
                        log::info!("Resolved a new service: {}", info.get_fullname());
                        let full_name = info.get_fullname().to_string();
                        let addresses: Vec<Ipv4Addr> = info.get_addresses_v4().iter().map(|addr| **addr).collect();
                        cat_reminder_instances.insert(full_name, addresses);
                        network_tx.send(TransportEvent::NodeListUpdated(cat_reminder_instances.clone())).expect("Failed to send updated cat reminder instances");
                    }
                }
                ServiceEvent::ServiceRemoved(removed_service_type, full_name) => {
                    if removed_service_type == SERVICE_TYPE {
                        log::info!("Removed service on: {}", full_name);
                        cat_reminder_instances.remove(&full_name);
                        network_tx.send(TransportEvent::NodeListUpdated(cat_reminder_instances.clone())).expect("Failed to send updated cat reminder instances");
                    }
                }
                _ => {
                    if shutdown_flag.load(Ordering::Relaxed) {
                        break;
                    }

                }
            }
        }
        mdns.unregister(&service_fullname).unwrap();
        let _ = mdns.shutdown();
    });
}
