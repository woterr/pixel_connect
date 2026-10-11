#[macro_use]
extern crate log;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::anyhow;
use channel::ChannelMessage;
#[cfg(all(feature = "experimental", target_os = "linux"))]
use hdl::BleAdvertiser;
use hdl::{ReceiverAdvertiser, ReceiverGattServer};
use hdl::MDnsDiscovery;
use once_cell::sync::Lazy;
use rand::distr::Alphanumeric;
use rand::Rng;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[cfg(feature = "experimental")]
use crate::hdl::BleListener;
use crate::hdl::MDnsServer;
use crate::manager::TcpServer;

pub mod channel;
mod errors;
mod hdl;
mod manager;
mod utils;

pub use hdl::{EndpointInfo, OutboundPayload, State, Visibility};
pub use manager::SendInfo;
pub use utils::DeviceType;

pub mod sharing_nearby {
    include!(concat!(env!("OUT_DIR"), "/sharing.nearby.rs"));
}

pub mod securemessage {
    include!(concat!(env!("OUT_DIR"), "/securemessage.rs"));
}

pub mod securegcm {
    include!(concat!(env!("OUT_DIR"), "/securegcm.rs"));
}

pub mod location_nearby_connections {
    include!(concat!(env!("OUT_DIR"), "/location.nearby.connections.rs"));
}

static CUSTOM_DOWNLOAD: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

#[derive(Debug)]
pub struct QuickShareService {
    tracker: Option<TaskTracker>,
    ctoken: Option<CancellationToken>,
    // Discovery token is different than ctoken because he is on his own
    // - can be cancelled while the ctoken is still active
    discovery_ctk: Option<CancellationToken>,

    // Used to trigger a change in the mDNS visibility (and later on, BLE)
    pub visibility_sender: Arc<Mutex<watch::Sender<Visibility>>>,
    visibility_receiver: watch::Receiver<Visibility>,

    // Only used to send the info "a nearby device is sharing"
    ble_sender: broadcast::Sender<()>,

    port_number: Option<u32>,

    pub message_sender: broadcast::Sender<ChannelMessage>,

    // Kept so the application layer can request outbound transfers after the
    // service has been moved into its runtime-independent state.
    send_sender: Option<mpsc::Sender<SendInfo>>,
}

impl Default for QuickShareService {
    fn default() -> Self {
        Self::new(Visibility::Visible, None, None)
    }
}

impl QuickShareService {
    pub fn new(
        visibility: Visibility,
        port_number: Option<u32>,
        download_path: Option<PathBuf>,
    ) -> Self {
        let mut guard = CUSTOM_DOWNLOAD.write().unwrap();
        *guard = download_path;

        let (message_sender, _) = broadcast::channel(50);
        let (ble_sender, _) = broadcast::channel(5);

        // Define default visibility as per the args inside the new()
        let (visibility_sender, visibility_receiver) = watch::channel(Visibility::Invisible);
        let _ = visibility_sender.send(visibility);

        Self {
            tracker: None,
            ctoken: None,
            discovery_ctk: None,
            visibility_sender: Arc::new(Mutex::new(visibility_sender)),
            visibility_receiver,
            ble_sender,
            port_number,
            message_sender,
            send_sender: None,
        }
    }

    pub async fn run(
        &mut self,
    ) -> Result<(mpsc::Sender<SendInfo>, broadcast::Receiver<()>), anyhow::Error> {
        let tracker = TaskTracker::new();
        let ctoken = CancellationToken::new();
        self.tracker = Some(tracker.clone());
        self.ctoken = Some(ctoken.clone());

        let endpoint_id: Vec<u8> = rand::rng()
            .sample_iter(Alphanumeric)
            .take(4)
            .map(u8::from)
            .collect();
        let tcp_listener =
            TcpListener::bind(format!("0.0.0.0:{}", self.port_number.unwrap_or(0))).await?;
        let binded_addr = tcp_listener.local_addr()?;
        info!("TcpListener on: {}", binded_addr);

        // MPSC for the TcpServer
        let send_channel = mpsc::channel(10);
        // Start TcpServer in own "task"
        let mut server = TcpServer::new(
            endpoint_id[..4].try_into()?,
            tcp_listener,
            self.message_sender.clone(),
            send_channel.1,
        )?;
        let ctk = ctoken.clone();
        tracker.spawn(async move { server.run(ctk).await });

        #[cfg(feature = "experimental")]
        {
            // Don't threat BleListener error as fatal, it's a nice to have.
            if let Ok(ble) = BleListener::new(self.ble_sender.clone()).await {
                let ctk = ctoken.clone();
                tracker.spawn(async move { ble.run(ctk).await });
            }
        }

        // Quick Share *receiver* discovery over BLE.
        //
        // Google Quick Share drops the phone off Wi-Fi while it browses, so an
        // mDNS-only receiver is invisible during exactly the moment the user is
        // looking for targets. The phone is then left holding only a BLE
        // endpoint, which is what these two serve.
        //
        // The `endpoint_id` must be the same four bytes mDNS uses, or the two
        // resolve to two different endpoints and the phone sees a target it
        // cannot reach.
        let rx_endpoint_id: [u8; 4] = endpoint_id[..4].try_into()?;
        {
            let ctk_ble = ctoken.clone();
            // A dedicated channel, NOT self.message_sender.
            //
            // Every InboundRequest subscribes to the channel it was given and
            // each `recv()` takes the *first* message, so two sessions sharing
            // one channel steal each other's traffic. With BLE and TCP inbound
            // both live, an AcceptTransfer sent for one could be consumed by the
            // other - which is what made "Accept" in the notification appear to
            // do nothing.
            let (msg_sender, _) = broadcast::channel(50);
            let hostname = sys_metrics::host::get_hostname().unwrap_or_else(|_| "Pixel Connect".into());
            let endpoint_for_ble = rx_endpoint_id;
            let hostname_gatt = hostname.clone();
            tracker.spawn(async move {
                // GATT first: a connectable advertisement is only useful once
                // something is there to connect to.
                // slot0 returns exactly the bytes the advertisement carries.
                let advertisement =
                    hdl::receiver_service_data(endpoint_for_ble, 3, &hostname_gatt);
                match ReceiverGattServer::new(
                    advertisement,
                    msg_sender.clone(),
                    0,
                )
                .await
                {
                    Ok(gatt) => {
                        if let Err(e) = gatt.run(ctk_ble.clone()).await {
                            error!("ReceiverGattServer ended: {e}");
                        }
                    }
                    Err(e) => error!(
                        "could not start the Quick Share BLE GATT service, so the \
                         phone cannot discover this machine over Bluetooth: {e}"
                    ),
                }
            });
            // Advertising 0xFEF3 makes the phone prefer BLE, and Quick Share
            // then sticks at "connecting" because the weave payload path is not
            // finished. That regressed the working mDNS/LAN transfer for
            // everyone, so the advertisement is opt-in until the data path
            // works.
            //
            // The GATT service above stays registered either way: it costs
            // nothing and is what a completed implementation needs.
            //
            // PIXEL_CONNECT_BLE_RX=1 re-enables it for testing.
            let ble_rx = std::env::var("PIXEL_CONNECT_BLE_RX")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if !ble_rx {
                info!(
                    "Quick Share BLE receiver advertisement is off (PIXEL_CONNECT_BLE_RX \
                     is not set); the phone will discover this machine over mDNS/LAN"
                );
            } else {
            let ctk_adv = ctoken.clone();
            let hostname_for_adv = hostname.clone();
            tracker.spawn(async move {
                match ReceiverAdvertiser::new(endpoint_for_ble, 3, &hostname_for_adv).await {
                    Ok(adv) => {
                        if let Err(e) = adv.run(ctk_adv).await {
                            error!("ReceiverAdvertiser ended: {e}");
                        }
                    }
                    Err(e) => error!("could not start the Quick Share BLE advertiser: {e}"),
                }
            });
            }
        }

        // Start MDnsServer in own "task"
        let mut mdns = MDnsServer::new(
            endpoint_id[..4].try_into()?,
            binded_addr.port(),
            self.ble_sender.subscribe(),
            self.visibility_sender.clone(),
            self.visibility_receiver.clone(),
        )?;
        let ctk = ctoken.clone();
        tracker.spawn(async move { mdns.run(ctk).await });

        tracker.close();

        self.send_sender = Some(send_channel.0.clone());
        Ok((send_channel.0, self.ble_sender.subscribe()))
    }

    /// The channel used to request an outbound transfer, once `run()` has
    /// completed.
    pub fn send_receiver(&self) -> Option<mpsc::Sender<SendInfo>> {
        self.send_sender.clone()
    }

    /// A fresh receiver for BLE "a device is sharing nearby" hints.
    pub fn subscribe_ble(&self) -> broadcast::Receiver<()> {
        self.ble_sender.subscribe()
    }

    pub fn discovery(
        &mut self,
        sender: broadcast::Sender<EndpointInfo>,
    ) -> Result<(), anyhow::Error> {
        let tracker = self
            .tracker
            .as_ref()
            .ok_or_else(|| anyhow!("The service wasn't first started"))?;

        let ctk = CancellationToken::new();
        self.discovery_ctk = Some(ctk.clone());

        #[cfg(all(feature = "experimental", target_os = "linux"))]
        {
            let ctk_blea = ctk.clone();
            tracker.spawn(async move {
                let blea = match BleAdvertiser::new().await {
                    Ok(b) => b,
                    Err(e) => {
                        error!("Couldn't init BleAdvertiser: {}", e);
                        return;
                    }
                };

                if let Err(e) = blea.run(ctk_blea).await {
                    error!("Couldn't start BleAdvertiser: {}", e);
                }
            });
        }

        let discovery = MDnsDiscovery::new(sender)?;
        tracker.spawn(async move { discovery.run(ctk.clone()).await });

        Ok(())
    }

    pub fn stop_discovery(&mut self) {
        if let Some(discovert_ctk) = &self.discovery_ctk {
            discovert_ctk.cancel();
            self.discovery_ctk = None;
        }
    }

    pub fn change_visibility(&mut self, nv: Visibility) {
        self.visibility_sender
            .lock()
            .unwrap()
            .send_modify(|state| *state = nv);
    }

    pub async fn stop(&mut self) {
        self.stop_discovery();

        if let Some(ctoken) = &self.ctoken {
            ctoken.cancel();
        }

        if let Some(tracker) = &self.tracker {
            tracker.wait().await;
        }

        self.ctoken = None;
        self.tracker = None;
    }

    // Setting None here will resume the default settings
    pub fn set_download_path(&self, p: Option<PathBuf>) {
        debug!("Setting the download path to {:?}", p);
        let mut guard = CUSTOM_DOWNLOAD.write().unwrap();
        *guard = p;
    }
}
