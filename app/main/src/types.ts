// Pixel Connect — backend types.
//
// These mirror the Rust structures in `src-tauri/src` and
// `core/kdeconnect/src/types.rs`. Keep them in sync when the backend changes.

export type PairState =
	| 'NotPaired'
	| 'Requested'
	| 'RequestedByPeer'
	| 'Paired'

export type LinkState =
	| 'Unpaired'
	| 'PairingRequested'
	| 'PairingRequestedByPeer'
	| 'Disconnected'
	| 'Connected'
	| 'Connecting'

export type Transport = 'lan' | 'bluetooth' | 'wifiDirect' | 'none'

/** `available` means implemented *and* exercised against a real peer. */
export type TransportState = 'available' | 'unverified' | 'unavailable'

export interface TransportStatus {
	transport: Transport
	name: string
	state: TransportState
	summary: string
	blocker?: string
	carriesData: boolean
}

export type DeviceType = 'desktop' | 'laptop' | 'phone' | 'tablet' | 'tv'

export interface KcDevice {
	id: string
	name: string
	type: DeviceType
	protocolVersion: number
	pairState: PairState
	linkState: LinkState
	transport: Transport
	address: string | null
	incomingCapabilities: string[]
	outgoingCapabilities: string[]
	features: string[]
	trusted: boolean
	verificationKey: string | null
	protocolError: string | null
	lastSeen: number
	batteryLevel: number | null
	batteryIsCharging: boolean | null
}

export interface DeviceInfo {
	id: string
	name: string
	device_type: DeviceType
	protocol_version: number
	incoming_capabilities: string[]
	outgoing_capabilities: string[]
}

export interface Settings {
	realclose: boolean
	startminimized: boolean
	autostart: boolean
	visibility: number
	port: number | null
	downloadPath: string | null
	incomingPath: string | null
	debugLevel: string | null
	clipboardEnabled: boolean
	clipboardAutoShare: boolean
	kdeDeviceName: string
	nautilusEnabled: boolean
	preferredDeviceId: string | null
}

export interface AppInfo {
	name: string
	version: string
	identifier: string
	kdeconnectProtocol: number
	quickshareService: boolean
	clipboardBackend: string
	filemanagerSocket: string
}

export interface PendingShare {
	id: string
	deviceId: string
	deviceName: string
	kind: 'file' | 'text' | 'url'
	filename: string | null
	filesize: number | null
	text: string | null
	url: string | null
	numberOfFiles: number | null
	totalPayloadSize: number | null
}

export interface RemoteNotification {
	/** `<deviceId>:<notificationId>`, so ids from two phones never collide. */
	id: string
	title: string
	text: string
	appName: string
	timestamp: number
	/** Action titles, in the order the phone offered them. */
	actions: string[]
	/** Present when the notification accepts a typed reply. */
	requestReplyId?: string
	isClearable: boolean
	silent: boolean
	groupName?: string
}

export interface TransferProgress {
	id: string
	deviceId: string
	filename: string
	bytes: number
	total: number
	done: boolean
	error: string | null
}

export interface BatteryState {
	deviceId: string
	level: number
	charging: boolean
}

export interface ConnectivityState {
	deviceId: string
	wifiStrength: number | null
	wifiName: string | null
	bluetoothStrength: number | null
	cellularStrength: number | null
	cellularName: string | null
}

export type ConnectionKind = 'usb' | 'wireless' | 'emulator'

export interface AdbDevice {
	serial: string
	state: string
	kind: ConnectionKind
	model: string | null
	product: string | null
	device: string | null
	transportId: string | null
	label: string
	scrcpyReady: boolean
	scrcpyBlockReason: string | null
}

export interface ToolAvailability {
	adbPath: string | null
	scrcpyPath: string | null
	adbVersion: string | null
	scrcpyVersion: string | null
	error: string | null
}

export interface ScrcpyLaunch {
	serial: string
	args: string[]
	command: string
}

export type AuthMethod = 'agent' | 'identityFile' | 'password'

export interface SshTarget {
	id: string
	host: string
	port: number
	username: string
	auth_method: AuthMethod
	identity_file: string | null
	label: string | null
	confirmed: boolean
}

export interface SshProbe {
	targetId: string
	available: boolean
	version: string | null
	message: string
}

export interface FileManagerStatus {
	socket: string
	listening: boolean
	pythonExtensionInstalled: boolean
	scriptInstalled: boolean
	note: string
}

/** How the clipboard can be reached on this session. */
export type ClipboardMechanism =
	| 'waylandDataControl'
	| 'x11'
	| 'waylandX11Bridge'
	| 'waylandCoreOnly'
	| 'none'

export interface ClipboardCapabilities {
	mechanism: ClipboardMechanism
	mechanismLabel: string
	mechanismDetail: string
	wayland: boolean
	x11: boolean
	/** An unfocused process may read the clipboard. */
	backgroundReadAllowed: boolean
	/** An unfocused process may own the clipboard. */
	backgroundWriteAllowed: boolean
	note: string
}

// Quick Share (rquickshare) channel types.
export type ChannelDirection = 'FrontToLib' | 'LibToFront'
export type ChannelAction = 'AcceptTransfer' | 'RejectTransfer' | 'CancelTransfer'
export type QuickShareState =
	| 'Initial'
	| 'ReceivedConnectionRequest'
	| 'SentUkeyServerInit'
	| 'SentUkeyClientInit'
	| 'SentUkeyClientFinish'
	| 'SentPairedKeyEncryption'
	| 'ReceivedUkeyClientFinish'
	| 'SentConnectionResponse'
	| 'SentPairedKeyResult'
	| 'SentIntroduction'
	| 'ReceivedPairedKeyResult'
	| 'WaitingForUserConsent'
	| 'ReceivingFiles'
	| 'SendingFiles'
	| 'Disconnected'
	| 'Rejected'
	| 'Cancelled'
	| 'Finished'

export interface TransferMetadata {
	id: string
	source: { name: string; device_type: DeviceType } | null
	pin_code: string | null
	destination: string | null
	files: string[] | null
	text_type: string | null
	text_description: string | null
	text_payload: string | null
	total_bytes: number
	ack_bytes: number
}

export interface ChannelMessage {
	id: string
	direction: ChannelDirection
	action: ChannelAction | null
	rtype: 'Inbound' | 'Outbound' | null
	state: QuickShareState | null
	meta: TransferMetadata | null
}

export interface EndpointInfo {
	name: string
	device_type: DeviceType
	address: string | null
	port: number | null
}

export interface OutboundPayload {
	payloadType: number
	meta: TransferMetadata | null
	textPayload: string | null
	files: string[] | null
	visibility: number
}

export interface SendInfo {
	id: string
	name: string
	addr: string
	ob: OutboundPayload
}

export type Visibility = 0 | 1 | 2