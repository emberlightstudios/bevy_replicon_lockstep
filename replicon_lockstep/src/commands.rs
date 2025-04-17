use bevy::{
    prelude::*, reflect::MaybeTyped, utils::hashbrown::HashMap
};
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use types::MoveCommand;
use crate::prelude::*;

pub(crate) mod serialization;
pub mod types;

pub(crate) struct LockstepCommandsPlugin;

impl Plugin for LockstepCommandsPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<LockstepGameCommandBuffer>()
            .init_resource::<LockstepGameCommandsReceived>()
            .add_server_trigger_with::<ServerSendCommands>(
                Channel::Ordered, 
                serialization::serialize_server_send_commands,
                serialization::deserialize_server_send_commands,
            )
            .add_client_trigger_with::<ClientSendCommands>(
                Channel::Ordered, 
                serialization::serialize_client_send_commands,
                serialization::deserialize_client_send_commands,
            )
            .add_observer(receive_commands_server)
            .add_observer(send_empty_commands_to_server_on_tick)
            .add_systems(OnEnter(SimulationState::Running), send_initial_commands_to_server);
    }
}

/// Implement thsi trait for all command structs
//#[reflect_trait]
//pub trait LockstepCommandPayload: PartialReflect + Send + Sync {
//    fn clone_box(&self) -> Box<dyn LockstepCommandPayload>;
//}

/// Auto impl trait if trait bounds satisfy
/// Users can simply implement command payloads as PartialReflect + Clone
//impl<T> LockstepCommandPayload for T
//where
//    T: PartialReflect + Send + Sync + Clone,
//{
//    fn clone_box(&self) -> Box<dyn LockstepCommandPayload> {
//        Box::new(self.clone())
//    }
//}
//
//impl Clone for Box<dyn LockstepCommandPayload> {
//    fn clone(&self) -> Self {
//        self.clone_box()
//    }
//}

/// An event type for clients to send their commands for their current tick to the server
#[derive(Event, Default, TypePath)]
pub struct ClientSendCommands {
    pub issued_tick: SimTick,
    pub commands: Vec<Box<dyn PartialReflect>>,
}

impl Clone for ClientSendCommands {
    fn clone(&self) -> Self {
        Self {
            issued_tick: self.issued_tick.clone(),
            commands: self.commands.iter().map(|x| x.clone_value()).collect(),
        }
    }
}

/// An event type for the server to broadcast client commands with delayed tick
#[derive(Event, Default)]
pub(crate) struct ServerSendCommands {
    pub(crate) tick: SimTick,
    pub(crate) commands: LockstepClientCommands,
}

/// A type for storing per-client commands for one tick
#[derive(Default, Deref, DerefMut)]
pub struct LockstepClientCommands(HashMap<ClientId, Vec<Box<dyn PartialReflect>>>);

impl Clone for LockstepClientCommands {
    fn clone(&self) -> Self {
        Self(
            self.0
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.iter().map(|x| x.clone_value()).collect(),
                    )
                }).collect()
        )
    }
}


/// The client sends commands to the server and they get stored in this buffer
/// based on the tick they received from the client, i.e. the tick when issued.
/// This is only used on the server.  It's sole purpose is to track who is still 
/// sending data currently so that we can detect disconnects.
#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct LockstepGameCommandsReceived(HashMap<SimTick, LockstepClientCommands>);

/// This is similar to LockstepGameCommandsReceived. The difference is that
/// this is used on both clients and the server, and the tick keys have been delayed.
/// The server broadcasts commands to clients and they get stored in this buffer.
/// Inputs have client delays added to the tick to account for the ping of each client.
/// Users should implement systems in FixedUpdate to handle these commands.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct LockstepGameCommandBuffer(HashMap<SimTick, LockstepClientCommands>);

/// The server ticks only if it gets commands from all clients,
/// but by default clients only send commands when the server ticks.
/// This system sends an initial empty command queue on tick 0
/// just to get the party started
fn send_initial_commands_to_server(
    mut commands: Commands,
) {
    trace!("Sending intitial commands");
    commands.client_trigger(ClientSendCommands::default());
}

/// Commands won't be sent for every player on every tick.
/// Make sure we at least send empty commands on each tick to let
/// the server know we are still in the game
fn send_empty_commands_to_server_on_tick(
    tick: Trigger<ServerSendCommands>,
    mut commands: Commands,
    sim_tick: Res<SimulationTick>,
    local_client: Query<&LocalClient>,
) {
    // Dont send commands if in dedicated server mode
    if local_client.get_single().is_err() { return }

    trace!("tick changed to {}, sending comamnds", **sim_tick);
    commands.client_trigger(ClientSendCommands {
        issued_tick: tick.tick,
        ..default()
    });
}

/// When the server receives commmands from a client it should
///  - store the commands in the command history
///  x broadcast them to all other clients  (moved commands into tick event)
fn receive_commands_server(
    trigger: Trigger<FromClient<ClientSendCommands>>,
    mut received: ResMut<LockstepGameCommandsReceived>,
    mut history: ResMut<LockstepGameCommandBuffer>,
    current_tick: Res<SimulationTick>,
    clients: Query<&NetworkId>,
    settings: Res<SimulationSettings>,
    stats: Query<&NetworkStats>,
) { 
    // In host server mode, the server can send events to itself
    // Server sent events use Entity::PLACEHOLDER
    // Instead I have set Host to have its own entity which has NetworkId=1
    let client_id: u64 = clients.get(trigger.client_entity).map_or(1, |id: &NetworkId| id.get());
    let client_commands: &Vec<Box<dyn PartialReflect>> = &trigger.event().commands;
    let num_commands = client_commands.iter().len();
    trace!("server received commands from client {} for tick {}", client_id, trigger.event().issued_tick);

    // Track received commands always, even when empty, for managing connections
    let tick = trigger.event().issued_tick;
    received.entry(tick)
        .or_insert_with(LockstepClientCommands::default)
        .entry(client_id)
        .insert(client_commands.iter().map(|x| x.clone_value()).collect());

    // But only send valid commands back to clients
    if num_commands > 0 {
        // Input tick delay depends on ping, for host server default to 1 tick for now
        let tick_delay: u32 = stats
            .get(trigger.client_entity)
            .map_or(1, |s: &NetworkStats| ((s.rtt / 2.0) / settings.tick_timestep.as_secs_f64()).ceil() as SimTick);
        let execution_tick = **current_tick + tick_delay + settings.base_input_tick_delay as SimTick;
        trace!("storing commands for execution tick {} for client {}", execution_tick, client_id);
        history.entry(execution_tick)
            .or_insert_with(LockstepClientCommands::default)
            .entry(client_id)
            .insert(client_commands.iter().map(|x| x.clone_value()).collect());

    }
}