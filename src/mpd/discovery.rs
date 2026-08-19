//! Bonjour discovery of `_mpd._tcp` servers via mdns-sd.

use super::client::{DiscoveredServer, MpdEvent};
use futures::channel::mpsc::UnboundedSender;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

const SERVICE_TYPE: &str = "_mpd._tcp.local.";

pub struct Discovery {
    daemon: ServiceDaemon,
    stopped: Arc<AtomicBool>,
}

impl Discovery {
    /// Starts browsing; snapshots of the current server list are delivered as
    /// `MpdEvent::Discovered` whenever it changes.
    pub fn start(events: UnboundedSender<MpdEvent>) -> Option<Discovery> {
        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[discovery] mdns daemon failed: {e}");
                return None;
            }
        };
        let receiver = match daemon.browse(SERVICE_TYPE) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[discovery] browse failed: {e}");
                return None;
            }
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped2 = stopped.clone();
        thread::Builder::new()
            .name("mpd-discovery".into())
            .spawn(move || {
                let mut found: BTreeMap<String, DiscoveredServer> = BTreeMap::new();
                while let Ok(event) = receiver.recv() {
                    if stopped2.load(Ordering::SeqCst) {
                        break;
                    }
                    let changed = match event {
                        ServiceEvent::ServiceResolved(info) => {
                            let fullname = info.get_fullname().to_owned();
                            let name = instance_name(&fullname);
                            let host = info
                                .get_addresses_v4()
                                .into_iter()
                                .next()
                                .map(|ip| ip.to_string())
                                .or_else(|| {
                                    info.get_addresses().iter().next().map(|ip| ip.to_string())
                                })
                                .unwrap_or_else(|| {
                                    info.get_hostname().trim_end_matches('.').to_owned()
                                });
                            found.insert(
                                fullname,
                                DiscoveredServer {
                                    name,
                                    host,
                                    port: info.get_port(),
                                },
                            );
                            true
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            found.remove(&fullname).is_some()
                        }
                        _ => false,
                    };
                    if changed {
                        let mut list: Vec<_> = found.values().cloned().collect();
                        list.sort_by(|a, b| a.name.cmp(&b.name));
                        if events.unbounded_send(MpdEvent::Discovered(list)).is_err() {
                            break;
                        }
                    }
                }
            })
            .ok()?;
        Some(Discovery { daemon, stopped })
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.daemon.stop_browse(SERVICE_TYPE);
        let _ = self.daemon.shutdown();
    }
}

/// "My MPD._mpd._tcp.local." → "My MPD"
fn instance_name(fullname: &str) -> String {
    let raw = fullname
        .strip_suffix(SERVICE_TYPE)
        .map(|s| s.trim_end_matches('.'))
        .unwrap_or(fullname);
    // mdns-sd escapes spaces as "\032" in some code paths.
    raw.replace("\\032", " ").replace("\\.", ".")
}
