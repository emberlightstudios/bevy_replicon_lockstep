use std::net::Ipv4Addr;

use bevy::prelude::*;
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
            .add_client_trigger::<ClientReadyEvent>(Channel::Unordered)
            .add_client_trigger::<LocalClientIdRequestEvent>(Channel::Unordered)
            .add_server_trigger::<LocalClientIdResponseEvent>(Channel::Unordered)
            .add_systems(FixedPreUpdate, (
                check_all_clients_ready
                    .run_if(in_state(SimulationState::Setup).and(server_or_singleplayer)),
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
pub struct ServerSettings {
    pub server_mode: ServerMode,
    pub address: Ipv4Addr,
    pub port: u16,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            server_mode: ServerMode::Host,
            address: Ipv4Addr::LOCALHOST,
            port: 15342,
        }
    }
}

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

fn on_client_connect(
    trigger: Trigger<OnAdd, NetworkId>,
    ids: Query<&NetworkId>,
    local_client: Query<&LocalClient>,
    server: Res<RepliconServer>,
    server_settings: Res<ServerSettings>,
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

fn on_client_requested_id (
    trigger: Trigger<FromClient<LocalClientIdRequestEvent>>,
    network_ids: Query<(Entity, &NetworkId)>,
    mut commands: Commands,
) {
    let Ok((client, client_id)) = network_ids.get(trigger.client_entity)
        else { panic!("Failed to find client entity on new connection") };
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
            commands.entity(host_entity).insert(ClientReady);
        }
    } else {
        commands.entity(ready.client_entity).insert(ClientReady);
    }
}

fn check_all_clients_ready(
    not_ready: Query<Entity, (With<NetworkId>, Without<ClientReady>)>,
    mut commands: Commands,
) {
    // Need to check for disconnections in the Setup state.  This will miss them.
    if not_ready.is_empty() {
        commands.server_trigger(ToClients {
            mode: SendMode::Broadcast,
            event: SetSimulationState(SimulationState::Starting),
        });
    }
}