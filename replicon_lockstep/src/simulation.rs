use bevy::prelude::*;
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use std::time::Duration;
use serde::{Serialize, Deserialize};
use crate::{commands::{LockstepGameCommandBuffer, LockstepGameCommandsReceived}, connections::ClientReady, prelude::ClientReadyEvent};

pub type SimTick = u32;

pub(crate) struct LockstepSimulationPlugin;

#[derive(SystemSet, Debug, Eq, PartialEq, Clone, Hash)]
pub struct SimulationTickSystemSet;

impl Plugin for LockstepSimulationPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_state(SimulationState::None)
            .add_observer(handle_sim_state_change)
            .add_server_event::<SimulationTickEvent>(Channel::Ordered)
            .add_server_trigger::<SetSimulationState>(Channel::Ordered)
            .add_client_trigger::<ClientReadyEvent>(Channel::Unordered)
            .add_systems(FixedPostUpdate, 
                tick_server
                    .run_if(server_running.and(in_state(SimulationState::Running)))
                    .before(ServerSet::Send)
            )
            .add_systems(FixedPreUpdate, 
                tick_client
                    .run_if(client_connected.and(in_state(SimulationState::Running)).and(not(server_running)))
                    .after(ClientSet::Receive)
                    .in_set(SimulationTickSystemSet)
            )
            .add_systems(OnEnter(SimulationState::Starting), 
                start_simulation
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
    /// There is no client-side prediction or extrapolation.  This is the
    /// base tick delay for all client inputs, regardless of ping. Round trip
    /// time gets added to this to determine the total input delay.  This 
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
    pub connection_check_tick_delay: u8,
    /// The number of tick equivalent timestaps we will wait for input
    /// before declaring a client is disconnected.  The simulation will be
    /// paused while waiting.
    pub disconnect_tick_threshold: u8,
}

impl Default for SimulationSettings {
    fn default() -> Self {
        Self {
            tick_timestep: Duration::from_millis(33),
            num_players: 8,
            base_input_tick_delay: 2,
            connection_check_tick_delay: 5,
            disconnect_tick_threshold: 10,
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

#[derive(Event, Serialize, Deserialize, Deref)]
pub struct SimulationTickEvent(SimTick);

#[derive(Resource, Deref, DerefMut, Default)]
pub struct SimulationTick(SimTick);

/// An event for the server to change the simulation state on the clients
#[derive(Event, Serialize, Deserialize, Deref)]
pub struct SetSimulationState(pub SimulationState);

fn handle_sim_state_change(
    trigger: Trigger<SetSimulationState>,
    mut sim_state: ResMut<NextState<SimulationState>>
) {
    info!("Simulation entering state {:#?}", trigger.0);
    sim_state.set(trigger.0);
}

/// Receives simulation tick events from the server
fn tick_client(
    mut ticks: EventReader<SimulationTickEvent>,
    mut sim_tick: ResMut<SimulationTick>,
) {
    for tick in ticks.read() {
        sim_tick.0 = tick.0;
    }
}

/// Handles incrementing the simulation tick on the server
fn tick_server(
    mut disconnect_timer: Local<u8>,
    mut next_state: ResMut<NextState<SimulationState>>,
    mut sim_tick: ResMut<SimulationTick>,
    mut tick_events: EventWriter<ToClients<SimulationTickEvent>>,
    clients: Query<&NetworkId>,
    stats: Query<&NetworkStats>,
    commands_received: Res<LockstepGameCommandsReceived>,
    settings: Res<SimulationSettings>,
) {
    let mut tick_delay = 0u32;
    if stats.iter().len() > 0 {  // True if clients connected
        // Before ticking the sim for connected clients, we need to check
        // client commands to make sure everyone is still connected and sending data. 
        // We don't want to check the current tick because the timestep may be 
        // smaller than the player's ping, so we check in the past based on the max rtt
        // Essentially, we are letting the server's sim run a few ticks before clients'
        // sims start so that clients are sufficiently behind the server's time once 
        // they start replicating each other's commands.
        tick_delay = (stats
                .iter()
                .max_by(|a, b| a.rtt.partial_cmp(&b.rtt).unwrap())
                .unwrap()
                .rtt / 2.0).ceil() as u32 + settings.connection_check_tick_delay as u32;
    }
    let mut tick_to_check = sim_tick.0;
    if tick_delay < tick_to_check { tick_to_check -= tick_delay }
    //trace!("checking inputs for tick {}. tick delay is {}", tick_to_check, tick_delay);

    if let Some(clients_for_tick) = commands_received.get(&tick_to_check) {
        if clients_for_tick.iter().len() == clients.iter().len() {
            sim_tick.0 += 1;
            trace!("ticked to {}", sim_tick.0);
            *disconnect_timer = 0;
            tick_events.send(ToClients { mode: SendMode::Broadcast, event: SimulationTickEvent(sim_tick.0) });
        } else {
            trace!("tick not ready");
            *disconnect_timer += 1;
            if *disconnect_timer > settings.disconnect_tick_threshold {
                *disconnect_timer = 0;
                info!("Simulation paused due to missing client commands. ");
                next_state.set(SimulationState::Paused)
            }
        }
    }
}

fn start_simulation(
    mut commands: Commands,
    mut command_buffer: ResMut<LockstepGameCommandBuffer>,
    mut command_history: ResMut<LockstepGameCommandsReceived>,
    ready: Query<Entity, With<ClientReady>>,
    server: Res<RepliconServer>,
) {
    commands.insert_resource(SimulationTick(0));
    command_buffer.clear();
    if server.is_running() {
        command_history.clear();
        commands.server_trigger(ToClients {
            mode: SendMode::Broadcast,
            event: SetSimulationState(SimulationState::Running),
        });
        for client in ready.iter() {
            commands.entity(client).remove::<ClientReady>();
        }
    }
}
