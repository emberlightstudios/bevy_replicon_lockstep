use bevy::prelude::*;
use bevy::utils::hashbrown::HashMap;
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use serde::{Serialize, Deserialize};
use crate::{prelude::*, commands::{ServerSendCommands, LockstepGameCommandsReceived}, connections::ClientReady};

pub type SimTick = u32;

pub(crate) struct LockstepSimulationPlugin;

impl Plugin for LockstepSimulationPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_state(SimulationState::None)
            .add_event::<SimulationTickUpdate>()
            .add_systems(OnEnter(SimulationState::Setup), setup_simulation)
            .add_systems(OnEnter(SimulationState::Starting), start_simulation)
            .add_systems(Update, cache_ids)
            .init_resource::<SimulationIdEntityMap>()
            .add_observer(handle_sim_state_change)
            .add_observer(tick_client)
            .add_server_trigger::<SetSimulationState>(Channel::Ordered)
            .add_client_trigger::<ClientReadyEvent>(Channel::Unordered)
            .register_type::<SimulationId>()
            .add_systems(FixedPostUpdate, 
                tick_server
                    .run_if(server_running.and(in_state(SimulationState::Running)))
                    .before(ServerSet::Send)
            );
    }
}

/// Parameters for the lockstep simulation
#[derive(Resource, Debug, Clone)]
pub struct SimulationSettings {
    /// The duration of each tick in the simulation
    pub tick_timestep: Duration,
    /// The expected number of players for the game 
    pub num_players: u8,
    /// Lockstep simulations have an inherent input lag.  The simulation is
    /// always executing commands issued in the past to avoid desyncs.
    /// There is no client-side prediction or extrapolation.  This field is the
    /// base tick delay for all client inputs, regardless of ping. Round trip
    /// time / 2 gets added to this to determine the total input delay.  This 
    /// parameter helps account for packet jitter.
    pub base_input_tick_delay: u8,
    /// The simulation checks for client inputs before proceeding to the 
    /// next tick. This is to ensure no clients have disconnected and are
    /// still in the game. But due to client ping it has to check some tick
    /// in the past. How far back it should check will depend on the max ping
    /// of all connected clients rounded up to the next tick. To that value
    /// will be added this value which will check even further in the past.
    /// This is also to account for packet jitter, but instead of delaying
    /// input execution, we are delaying disconnection signals.
    pub connection_check_tick_delay: u32,
    /// The number of tick equivalent timesteps we will wait for input
    /// before declaring a client is disconnected.  The simulation will be
    /// paused while waiting.
    pub disconnect_tick_threshold: u8,
}

impl Default for SimulationSettings {
    fn default() -> Self {
        Self {
            tick_timestep: Duration::from_millis(33),
            num_players: 8,
            base_input_tick_delay: 1,
            connection_check_tick_delay: 1,
            disconnect_tick_threshold: 20,
        }
    }
}

/// Different states for the simulation
#[derive(States, Debug, Hash, Eq, PartialEq, Copy, Clone, Serialize, Deserialize, Default)]
pub enum SimulationState {
    /// No simulation
    #[default]
    None,
    /// Clients are connecting to the server/host.
    Connecting,
    /// Clients have all connected.  Hook into this state to load assets, set up teams, etc.
    /// Once done, mark your local client as ready with the ClientReady marker component.
    Setup,
    /// Resources are being initialized for the simulation.
    Starting,
    /// The simulation is running.
    Running,
    /// A client is attempting to reconnect.
    Reconnecting,
    /// The simulation has paused.  Perhaps a client disconnected.
    Paused,
    /// The game has ended.  Cleanup operations go here.
    Ending,
}

/// An event for the server to change the simulation state on the clients
#[derive(Event, Serialize, Deserialize, Deref)]
pub struct SetSimulationState(pub SimulationState);

/// Changes the simlation state in response to server trigger
fn handle_sim_state_change(
    trigger: Trigger<SetSimulationState>,
    mut sim_state: ResMut<NextState<SimulationState>>
) {
    info!("Simulation entering state {:#?}", trigger.0);
    sim_state.set(trigger.0);
}

fn setup_simulation(
    mut commands: Commands,
    mut command_history: ResMut<LockstepGameCommandBuffer>,
    mut commands_received: ResMut<LockstepGameCommandsReceived>,
    mut id_entity_map: ResMut<SimulationIdEntityMap>,
) {
    commands.insert_resource(SimulationTick(0));
    command_history.clear();
    commands_received.clear();
    id_entity_map.clear();
    SIMULATION_ID_COUNTER.store(1, Ordering::SeqCst);
}

fn start_simulation(
    mut commands: Commands,
    ready: Query<Entity, With<ClientReady>>,
    server: Res<RepliconServer>,
) {
    if server.is_running() {
        commands.server_trigger(ToClients {
            mode: SendMode::Broadcast,
            event: SetSimulationState(SimulationState::Running),
        });
        for client in ready.iter() {
            commands.entity(client).remove::<ClientReady>();
        }
    }
}

/// Event triggered when the simulation ticks
#[derive(Event, Serialize, Deserialize, Deref)]
pub struct SimulationTickUpdate(pub SimTick);

/// The current simulation tick. Several ticks may arrive at once 
/// without sufficient time to process them all.  This is only used to record
/// commands received from the server.  Users should implement their own 
/// logic to track which commands need to be processed.  
#[derive(Resource, Deref, DerefMut, Default)]
pub struct SimulationTick(SimTick);

/// An atomic counter for incrementing the simulation id on each assignment
static SIMULATION_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Unique Id for each entity in the simulation 
#[derive(Component, Deref, Serialize, Deserialize, Debug, Clone, Copy, Reflect, Eq, PartialEq, Hash)]
pub struct SimulationId(u32);

impl SimulationId {
    // Use this when sending commands from clients
    pub const PLACEHOLDER: SimulationId = SimulationId(0);

    // Use this when implementing the commands after receiving from server
    pub fn new() -> Self {
        // What happens if someone manages to reach u32::MAX ?
        Self(SIMULATION_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Resource to map SimulationIds to Entities for quick look-up of entities
#[derive(Resource, Deref, DerefMut, Default)]
pub struct SimulationIdEntityMap(HashMap<SimulationId, Entity>);

fn cache_ids(
    new_ids: Query<(Entity, &SimulationId), Added<SimulationId>>,
    mut id_map: ResMut<SimulationIdEntityMap>,
) {
    new_ids.iter().for_each(|(entity, &id)| {
        id_map.insert(id, entity);
    })
}

/// Receives simulation tick events from the server.
fn tick_client(
    tick: Trigger<ServerSendCommands>,
    mut sim_tick: ResMut<SimulationTick>,
    mut command_history: ResMut<LockstepGameCommandBuffer>,
    mut sim_tick_event: EventWriter<SimulationTickUpdate>,
    server: Res<RepliconServer>,
) {
    if !server.is_running() {
        command_history.resize(tick.tick + 1, tick.commands.clone());
        trace!("Received tick {}", tick.tick);
        if tick.tick == sim_tick.0 + 1 || sim_tick.0 == 0 {
            sim_tick.0 = tick.tick;
        } else {
            panic!("Received ticks out of order");
        }
    }
    sim_tick_event.send(SimulationTickUpdate(tick.tick));
}

/// Handles incrementing the simulation tick on the server
fn tick_server(
    mut disconnect_timer: Local<u8>,
    mut next_state: ResMut<NextState<SimulationState>>,
    mut sim_tick: ResMut<SimulationTick>,
    mut commands: Commands,
    clients: Query<&NetworkId>,
    stats: Query<&NetworkStats>,
    commands_received: Res<LockstepGameCommandsReceived>,
    mut command_history: ResMut<LockstepGameCommandBuffer>,
    settings: Res<SimulationSettings>,
) {
    let mut tick_delay = 0u32;
    if stats.iter().len() > 0 {  // True if clients connected
        // Before ticking the sim for connected clients, we need to check received
        // client commands to make sure everyone is still connected and sending data. 
        // We don't want to check the current tick because the simulation timestep may be 
        // smaller than the players' ping, so we go back in the past based on the max rtt.
        // Essentially, we are letting the server's sim run a few ticks ahead of clients
        // so that clients are sufficiently behind the server's time once they start
        // replicating each other's commands.
        tick_delay = (stats
                .iter()
                .max_by(|a: &&NetworkStats, b: &&NetworkStats| a.rtt.partial_cmp(&b.rtt).unwrap())
                .unwrap()
                .rtt / 2.0).ceil() as u32 + settings.connection_check_tick_delay;
    }
    let mut tick_to_check = sim_tick.0;
    if tick_delay > tick_to_check {
        tick_to_check = 0
    } else {
        tick_to_check -= tick_delay
    }

    if let Some(clients_for_tick) = commands_received.get(tick_to_check) {
        if clients_for_tick.iter().len() == clients.iter().len() {
            sim_tick.0 += 1;
            trace!("ticked to {}", sim_tick.0);
            *disconnect_timer = 0;
            let tick_commands = command_history.get(sim_tick.0);
            commands.server_trigger(ToClients{
                mode: SendMode::Broadcast,
                event: ServerSendCommands {
                    tick: sim_tick.0,
                    commands: tick_commands.cloned().unwrap_or_else(|| {
                        let default = LockstepClientCommands::default();
                        if command_history.len() <= sim_tick.0 as usize {
                            command_history.resize(sim_tick.0, default.clone());
                        }
                        default
                    }),
                }
            });
        } else {
            trace!("tick not ready");
            *disconnect_timer += 1;
            if *disconnect_timer > settings.disconnect_tick_threshold {
                *disconnect_timer = 0;
                info!("Simulation paused due to missing client commands.");
                next_state.set(SimulationState::Paused);
                clients_for_tick
                            .iter()
                            .filter(|(c, _)| !clients_for_tick.contains_key(c))
                            .for_each(|(&c, _)| commands.trigger(ClientDisconnect(c)));
            }
        }
    }
}