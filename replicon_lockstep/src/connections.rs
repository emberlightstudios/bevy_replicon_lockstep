use std::{net::Ipv4Addr, time::Duration};
use bevy::{prelude::*, time::Stopwatch};
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use serde::{Deserialize, Serialize};
use crate::{prelude::{SimulationSettings, SimulationState}, simulation::SetSimulationState};


pub type ClientId = u64;

pub(crate) struct LockstepConnectionsPlugin;

impl Plugin for LockstepConnectionsPlugin {
    fn build(&self, app: &mut App) {
        app
            .replicate::<NetworkId>()
            .replicate::<ClientReady>()
            .add_observer(on_client_connect)
            .add_observer(on_client_requested_id)
            .add_observer(on_received_local_client_id)
            .add_observer(on_client_ready)
            .add_server_trigger::<LocalClientIdResponseEvent>(Channel::Unordered)
            .add_client_trigger::<LocalClientIdRequestEvent>(Channel::Unordered)
            .add_client_trigger::<ClientReadyEvent>(Channel::Unordered)
            .add_systems(FixedPreUpdate, (
                check_all_clients_ready
                    .run_if(in_state(SimulationState::Setup).and(server_running)),
                handle_local_client_disconnect
                    .run_if(not(server_running).and(not(client_connected)))
            ));
    }
}

#[derive(Default, Clone, PartialEq)]
pub enum ServerMode {
    #[default]
    Host,
    Dedicated,
}

#[derive(Resource, Clone)]
pub struct ConnectionSettings {
    pub server_mode: ServerMode,
    pub server_address: Ipv4Addr,
    pub server_port: u16,
    pub reconnect_timer: Duration,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        Self {
            server_mode: ServerMode::Host,
            server_address: Ipv4Addr::LOCALHOST,
            server_port: 15342,
            reconnect_timer: Duration::from_secs(5),
        }
    }
}

/// A trigger that fires when the local client should try to reconnect
#[derive(Event)]
pub struct ClientReconnect;

/// A trigger that fires when the client has disconnected 
/// Will be triggered on both the local client and the server
/// If on the local client, and not in the Ending state or
/// the None state it will first try trigger a reconnect event
/// and start a timer.  If the timer runs out this event fires.
#[derive(Event)]
pub struct ClientDisconnect(pub ClientId);

/// A trigger for the client to request the local client id from the server 
#[derive(Event, Serialize, Deserialize)]
struct LocalClientIdRequestEvent;

/// A trigger for the server to send the local client id to a connected client
#[derive(Event, Serialize, Deserialize, Deref)]
struct LocalClientIdResponseEvent(NetworkId);

/// Marker Component to identify the local client entity
#[derive(Component)]
pub struct LocalClient;

/// Marker Component to signal client has loaded and is ready to start the game
#[derive(Component, Serialize, Deserialize)]
pub struct ClientReady;

/// Event sent by clients to tell server to mark client as ready
#[derive(Event, Serialize, Deserialize)]
pub struct ClientReadyEvent;

/// Stopwatch for client reconnects
#[derive(Component, Deref, DerefMut, Default)]
struct ClientReconnectTimer {
    time: Stopwatch
}

fn on_client_connect(
    trigger: Trigger<OnAdd, NetworkId>,
    ids: Query<&NetworkId>,
    local_client: Query<&LocalClient>,
    server: Res<RepliconServer>,
    server_settings: Res<ConnectionSettings>,
    simulation_settings: Res<SimulationSettings>,
    mut commands: Commands,
) { 
    // If all players are connected begin the setup process.
    // You can hook into the Setup state to run systems to prepare
    // the game world before the game starts.  Send ClientReadyEvent
    // trigger when client setup is finished.
    if ids.iter().len() == simulation_settings.num_players as usize {
        commands.server_trigger(ToClients {
            mode: SendMode::Broadcast,
            event: SetSimulationState(SimulationState::Setup),
        });
    }

    // Host entity/id(1) will be spawned below when first client connects. 
    // We don't want to re-trigger the rest of this system when that happens
    if ids.get(trigger.entity()).unwrap().get() == 1 { return }

    if server.is_running() {
        // Replicate all remote client NetworkIds 
        commands.entity(trigger.entity()).insert(Replicated);

        if server_settings.server_mode == ServerMode::Host {
            // If no host entity exists yet (1st connection), create one
            if local_client.get_single().is_err() {
                commands.spawn((
                    NetworkId::new(1),
                    LocalClient,
                    Replicated,
                ));
            }
        }
    } else {
        // If we are a remote client and we don't know our local
        // client id, request it from the server, so we can apply the
        // LocalClient marker component.
        if local_client.is_empty() {
            commands.client_trigger(LocalClientIdRequestEvent);
        }
    }
}

/// Check the connection state
fn handle_local_client_disconnect(
    mut commands: Commands,
    mut state: ResMut<NextState<SimulationState>>,
    current_state: Res<State<SimulationState>>,
    mut timer: Query<(Entity, &mut ClientReconnectTimer)>,
    settings: Res<ConnectionSettings>,
    local_client: Query<&NetworkId, With<LocalClient>>,
    time: Res<Time<Fixed>>,
) {
    match *current_state.get() {
        SimulationState::Ending | SimulationState::None | SimulationState::Connecting => {
            return
        }
        SimulationState::Reconnecting => {
            let (entity, mut timer) = timer.single_mut();
            timer.tick(time.delta());
            if timer.elapsed() >= settings.reconnect_timer {
                commands.trigger(ClientDisconnect(local_client.single().get()));
                state.set(SimulationState::None);
                commands.entity(entity).despawn();
                info!("Client disconnected");
            }
        }
        _ => {
            info!("Disconnected from server.  Attempting to reconnect...");
            state.set(SimulationState::Reconnecting);
            commands.trigger(ClientReconnect);
            commands.spawn(ClientReconnectTimer{ time: Stopwatch::new() });
        }
    }
}

fn on_client_requested_id (
    trigger: Trigger<FromClient<LocalClientIdRequestEvent>>,
    network_ids: Query<(Entity, &NetworkId)>,
    mut commands: Commands,
) {
    let Ok((client, client_id)) = network_ids.get(trigger.client_entity)
        else { panic!("Failed to find client entity on new connection") };
    trace!("Client {} requested id. Sending", client_id.get());
    commands.server_trigger(ToClients {
        mode: SendMode::Direct(client),
        event: LocalClientIdResponseEvent(*client_id),
    });
}

fn on_received_local_client_id(
    local_client: Trigger<LocalClientIdResponseEvent>,
    mut commands: Commands,
    network_ids: Query<(Entity, &NetworkId)>,
) {
    trace!("Received local client id.");
    let local_client_id = **local_client;
    for (client, id) in network_ids.iter() {
        if *id == local_client_id {
            commands.entity(client).insert(LocalClient);
            return;
        }
    }
    panic!("failed to match local client");
}

fn on_client_ready (
    ready: Trigger<FromClient<ClientReadyEvent>>,
    host: Query<Entity, With<LocalClient>>,
    mut commands: Commands,
) {
    if ready.client_entity == Entity::PLACEHOLDER {
        // This is the host server triggering the event
        if let Ok(host_entity) = host.get_single() {
            trace!("host is ready");
            commands.entity(host_entity).insert(ClientReady);
        }
    } else {
        trace!("client {} is ready", ready.client_entity);
        commands.entity(ready.client_entity).insert(ClientReady);
    }
}

fn check_all_clients_ready(
    ids: Query<&NetworkId>,
    settings: Res<SimulationSettings>,
    not_ready: Query<Entity, (With<NetworkId>, Without<ClientReady>)>,
    mut commands: Commands,
) {
    if ids.iter().len() != settings.num_players as usize {
        panic!("Player(s) disconnected during setup phase.  Need to handle this.")
    }
    if not_ready.is_empty() {
        commands.server_trigger(ToClients {
            mode: SendMode::Broadcast,
            event: SetSimulationState(SimulationState::Starting),
        });
    }
}