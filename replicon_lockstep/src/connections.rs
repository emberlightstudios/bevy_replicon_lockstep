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
            .add_observer(replicate_network_ids)
            .add_observer(on_client_connected)
            .add_observer(on_client_ready)
            .add_observer(request_local_client_id)
            .add_observer(receive_local_client_id)
            .add_client_trigger::<ClientReadyEvent>(Channel::Unordered)
            .add_client_trigger::<LocalClientIdRequestEvent>(Channel::Unordered)
            .add_server_trigger::<LocalClientIdResponseEvent>(Channel::Unordered)
            .add_systems(FixedPreUpdate, (
                check_all_clients_ready
                    .run_if(in_state(SimulationState::Setup).and(server_or_singleplayer)),
            ));
    }
}

/// A trigger for the client to request the local client id from the server 
#[derive(Event, Serialize, Deserialize)]
struct LocalClientIdRequestEvent;

/// A trigger for the server to send the local client id to a connected client
#[derive(Event, Serialize, Deserialize, Deref)]
struct LocalClientIdResponseEvent(ClientId);

/// Marker Component to identify the local client entity
#[derive(Component)]
pub struct LocalClient;

/// Marker Component to signal client has loaded and is ready to start the game
#[derive(Component, Serialize, Deserialize)]
pub struct ClientReady;

/// Event sent by clients to tell server to mark client as ready
#[derive(Event, Serialize, Deserialize)]
pub struct ClientReadyEvent;

fn replicate_network_ids(
    trigger: Trigger<OnAdd, NetworkId>,
    mut commands: Commands,
) {
    commands.entity(trigger.entity()).insert(Replicated);
}

fn on_client_connected (
    trigger: Trigger<FromClient<LocalClientIdRequestEvent>>,
    network_ids: Query<(Entity, &NetworkId)>,
    settings: Res<SimulationSettings>,
    mut commands: Commands,
) {
    let Ok((client, client_id)) = network_ids.get(trigger.client_entity)
        else { panic!("Failed to find client entity on new connection") };
    if client_id.get() != 1 {
        info!("Sending local client id to {:?}", client_id.get());
        commands.server_trigger(ToClients {
            mode: SendMode::Direct(client),
            event: LocalClientIdResponseEvent(client_id.get()),
        });
        if network_ids.iter().len() == settings.num_players as usize {
            commands.server_trigger(ToClients {
                mode: SendMode::Broadcast,
                event: SetSimulationState(SimulationState::Setup),
            });
        }
    }
}

fn on_client_ready (
    ready: Trigger<FromClient<ClientReadyEvent>>,
    host: Query<Entity, With<LocalClient>>,
    mut commands: Commands,
) {
    info!("Received client ready trigger");
    if ready.client_entity == Entity::PLACEHOLDER {
        // Make sure we aren't a dedicated server
        // We must have a host client entity defined
        if let Ok(host_client) = host.get_single() {
            commands.entity(host_client).insert(ClientReady);
        }
    } else {
        commands.entity(ready.client_entity).insert(ClientReady);
    }
}

fn request_local_client_id(
    trigger: Trigger<OnAdd, NetworkId>,
    ids: Query<&NetworkId>,
    local_client: Query<&LocalClient>,
    mut commands: Commands,
) { 
    // currently this will make multiple requests, one for each connected client
    // until the response comes in.  not ideal but not a huge deal either
    if !local_client.is_empty() { return }
    // Don't trigger if we received the host client entity. 
    // This will have been set manually on the host
    if ids.get(trigger.entity()).unwrap().get() == 1 { return }
    commands.client_trigger(LocalClientIdRequestEvent);
}

fn receive_local_client_id(
    connected: Trigger<LocalClientIdResponseEvent>,
    mut commands: Commands,
    network_ids: Query<(Entity, &NetworkId)>,
) {
    if network_ids.iter().len() == 0 { return }
    let local_client_id = **connected;
    for (client, id) in network_ids.iter() {
        if id.get() == local_client_id {
            commands.entity(client).insert(LocalClient);
            return;
        }
    }
    panic!("failed to match local client");
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