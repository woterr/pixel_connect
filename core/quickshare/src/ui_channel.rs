// Pixel Connect — UI-facing Quick Share channel protocol.
//
// The engine underneath is `open-quickshare` (`crate::channel::Message`,
// `TransferAction`, `TransferState`), whose shapes differ from what the
// Tauri frontend speaks. This module is the UI contract and must not change
// without updating the frontend: `ChannelMessage`, `State` and
// `TransferMetadata` carry ts-rs bindings consumed by `app/main/src/types.ts`.
//
// Translation lives here too (`translate_*`), so the engine files stay
// verbatim upstream and the only adapted code is this boundary.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::hdl::info::{TransferMetadata as EngineMeta, TransferPayload, TransferPayloadKind};
use crate::hdl::TransferState as EngineState;
use crate::utils::RemoteDeviceInfo;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize, TS)]
#[ts(export)]
pub enum ChannelDirection {
    #[default]
    FrontToLib,
    LibToFront,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TS)]
#[ts(export)]
pub enum ChannelAction {
    AcceptTransfer,
    RejectTransfer,
    CancelTransfer,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize, TS)]
#[ts(export)]
pub enum TransferType {
    #[default]
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS, PartialEq)]
#[ts(export)]
pub enum State {
    #[default]
    Initial,
    ReceivedConnectionRequest,
    SentUkeyServerInit,
    SentUkeyClientInit,
    SentUkeyClientFinish,
    SentPairedKeyEncryption,
    ReceivedUkeyClientFinish,
    SentConnectionResponse,
    SentPairedKeyResult,
    SentIntroduction,
    ReceivedPairedKeyResult,
    WaitingForUserConsent,
    ReceivingFiles,
    SendingFiles,
    Disconnected,
    Rejected,
    Cancelled,
    Finished,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[ts(export)]
pub enum TextPayloadType {
    #[default]
    Url,
    Text,
    Wifi,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[ts(export)]
pub struct TransferMetadata {
    pub id: String,
    pub source: Option<RemoteDeviceInfo>,
    pub pin_code: Option<String>,

    pub destination: Option<String>,
    pub files: Option<Vec<String>>,

    pub text_type: Option<TextPayloadType>,
    pub text_description: Option<String>,
    pub text_payload: Option<String>,

    pub total_bytes: u64,
    pub ack_bytes: u64,

    /// Where received files were actually saved, filled in as each completes.
    ///
    /// The application cannot work this out for itself: the metadata carries
    /// only file *names*, and the download folder may be changed between the
    /// transfer starting and each file arriving.
    #[serde(default)]
    pub saved_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ChannelMessage {
    pub id: String,
    pub direction: ChannelDirection,

    // Only present when channelDirection is frontToLib
    pub action: Option<ChannelAction>,

    // Only present when channelDirection is libToFront
    pub rtype: Option<TransferType>,
    pub state: Option<State>,
    pub meta: Option<TransferMetadata>,
}

/// Engine state -> UI state. The variant sets are identical by construction
/// (both derive from the same protocol); a compile error here is the signal
/// that upstream added a state the UI must learn about.
pub fn translate_state(s: &EngineState) -> State {
    match s {
        EngineState::Initial => State::Initial,
        EngineState::ReceivedConnectionRequest => State::ReceivedConnectionRequest,
        EngineState::SentUkeyServerInit => State::SentUkeyServerInit,
        EngineState::SentUkeyClientInit => State::SentUkeyClientInit,
        EngineState::SentUkeyClientFinish => State::SentUkeyClientFinish,
        EngineState::SentPairedKeyEncryption => State::SentPairedKeyEncryption,
        EngineState::ReceivedUkeyClientFinish => State::ReceivedUkeyClientFinish,
        EngineState::SentConnectionResponse => State::SentConnectionResponse,
        EngineState::SentPairedKeyResult => State::SentPairedKeyResult,
        EngineState::SentIntroduction => State::SentIntroduction,
        EngineState::ReceivedPairedKeyResult => State::ReceivedPairedKeyResult,
        EngineState::WaitingForUserConsent => State::WaitingForUserConsent,
        EngineState::ReceivingFiles => State::ReceivingFiles,
        EngineState::SendingFiles => State::SendingFiles,
        EngineState::Disconnected => State::Disconnected,
        EngineState::Rejected => State::Rejected,
        EngineState::Cancelled => State::Cancelled,
        EngineState::Finished => State::Finished,
    }
}

/// Engine metadata -> UI metadata. Every UI field is derived; `saved_paths`
/// is filled by the engine (see the `saved_paths` field on the engine
/// metadata) because only the engine knows where each file landed.
pub fn translate_metadata(m: &EngineMeta) -> TransferMetadata {
    let (files, text_type, text_description, text_payload) = match &m.payload {
        None => match m.payload_kind {
            TransferPayloadKind::Files => (Some(Vec::new()), None, m.payload_preview.clone(), None),
            TransferPayloadKind::Text => (
                None,
                Some(TextPayloadType::Text),
                m.payload_preview.clone(),
                None,
            ),
            TransferPayloadKind::Url => (
                None,
                Some(TextPayloadType::Url),
                m.payload_preview.clone(),
                None,
            ),
            TransferPayloadKind::WiFi => (
                None,
                Some(TextPayloadType::Wifi),
                m.payload_preview.clone(),
                None,
            ),
        },
        Some(TransferPayload::Files(names)) => (Some(names.clone()), None, None, None),
        Some(TransferPayload::Text(t)) => (
            None,
            Some(TextPayloadType::Text),
            Some(t.clone()),
            Some(t.clone()),
        ),
        Some(TransferPayload::Url(u)) => (
            None,
            Some(TextPayloadType::Url),
            Some(u.clone()),
            Some(u.clone()),
        ),
        Some(TransferPayload::Wifi { ssid, .. }) => (
            None,
            Some(TextPayloadType::Wifi),
            Some(ssid.clone()),
            None,
        ),
    };
    // The engine reports text sub-types via its own enum; prefer it when the
    // payload itself does not pin one down.
    let text_type = text_type.or_else(|| match m.payload_kind {
        TransferPayloadKind::Text => Some(TextPayloadType::Text),
        TransferPayloadKind::Url => Some(TextPayloadType::Url),
        TransferPayloadKind::WiFi => Some(TextPayloadType::Wifi),
        TransferPayloadKind::Files => None,
    });
    TransferMetadata {
        id: m.id.clone(),
        source: m.source.clone(),
        pin_code: m.pin_code.clone(),
        destination: m.destination.clone(),
        files,
        text_type,
        text_description,
        text_payload,
        total_bytes: m.total_bytes,
        ack_bytes: m.ack_bytes,
        saved_paths: m.saved_paths.clone(),
    }
}

/// UI accept/reject/cancel -> engine action.
pub fn translate_action(a: &ChannelAction) -> crate::channel::TransferAction {
    match a {
        ChannelAction::AcceptTransfer => crate::channel::TransferAction::ConsentAccept,
        ChannelAction::RejectTransfer => crate::channel::TransferAction::ConsentDecline,
        ChannelAction::CancelTransfer => crate::channel::TransferAction::TransferCancel,
    }
}

/// An engine `Message::Client` event as the UI `ChannelMessage` the frontend
/// already speaks. Returns `None` for `Message::Lib`, which travels the other
/// way and is never forwarded to the UI.
pub fn translate_incoming(
    id: String,
    msg: &crate::channel::Message,
) -> Option<ChannelMessage> {
    match msg {
        crate::channel::Message::Lib { .. } => None,
        crate::channel::Message::Client(client) => Some(ChannelMessage {
            id,
            direction: ChannelDirection::LibToFront,
            action: None,
            rtype: Some(match client.kind {
                crate::channel::TransferKind::Inbound => TransferType::Inbound,
                crate::channel::TransferKind::Outbound => TransferType::Outbound,
            }),
            state: client.state.as_ref().map(translate_state),
            meta: client.metadata.as_ref().map(translate_metadata),
        }),
    }
}
