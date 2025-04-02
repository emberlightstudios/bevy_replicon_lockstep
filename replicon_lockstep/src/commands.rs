use bevy::{prelude::*, utils::hashbrown::{HashMap, HashSet}};
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use serde::{Deserialize, Serialize};
use crate::{prelude::*, simulation::{SimulationTickEvent, SimulationTickSystemSet}};

pub(crate) struct LockstepCommandsPlugin;

impl Plugin for LockstepCommandsPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<LockstepGameCommandBuffer>()
            .init_resource::<LockstepGameCommandsReceived>()
            .add_client_trigger::<ClientSendCommands>(Channel::Ordered)
            .add_server_trigger::<ServerSendCommands>(Channel::Ordered)
            .add_observer(receive_commands_server)
            .add_observer(receive_commands_client)
            .add_systems(OnEnter(SimulationState::Running), send_initial_commands_to_server)
            .add_systems(FixedPostUpdate,
                send_empty_commands_to_server_on_tick
                    .run_if(in_state(SimulationState::Running))
                    .after(SimulationTickSystemSet)
                    .before(ClientSet::Send)

            );
    }
}

/// The base type for commands which clients can send to the server.
/// The server will broadcast these to all connected clients.
/// If it fails to receive any from a client the simulation will change
/// state to paused and you can implement logic for reconnect, host migration etc.
/// 
/// Adding many command components to a large number of entities can incur
/// overhead in the ECS command buffer. Instead of putting these on each entity
/// and replicating, these commands are sent via events bidrectionally and can hold
/// a Vec<Entity> specifying which entities are issued this command.  This should
/// reduce network overhead I think.
/// 
/// Nevertheless it is implemented as a component so it can be added to entities
/// if desired. Although in this case it would save memory to set the entities field
/// to None.
/// 
/// This struct is intended to hold all necessary fields to specify everything
/// about the command.  More fields may need to be added in the future.
/// 
#[derive(Component, Debug, Serialize, Deserialize, Clone, Default)]
pub struct LockstepCommand {
    /// The id of the type of command
    pub command_type_id: u16,
    /// The entities which will carry out the command
    pub entities: Option<Vec<Entity>>,
    /// An optional follow-up command which should begin when this one ends
    pub then: Option<Box<LockstepCommand>>,
    /// A vec3 target for the command
    pub target_vector: Option<Vec3>,
    /// An entity target for the command
    pub target_entity: Option<Entity>,
}

/// This could probably be handled more idiomatically with some 
/// more advanced rust type shenanigans, but I coudn't figure out how.
/// Could use enums but it's not extensible for users of this crate.
/// Instead we will just use strings for now, but map them to u16 to 
/// keep serialization payload size down.
#[derive(Resource)]
pub struct CommandTypeRegistry {
    name_to_id: HashMap<String, u16>,
    id_to_name: HashMap<u16, String>,
}

impl CommandTypeRegistry {
    pub fn new(commands: Vec<String>) -> Self {
        let command_set: HashSet<String> = HashSet::from_iter(commands);
        let mut name_to_id = HashMap::<String, u16>::new();
        let mut id_to_name = HashMap::<u16, String>::new();
        for (i, name) in command_set.iter().enumerate() {
            name_to_id.insert(name.clone(), i as u16);
            id_to_name.insert(i as u16, name.clone());
        }
        CommandTypeRegistry {
            name_to_id,
            id_to_name,
        }
    }

    pub fn get_id(&self, name: String) -> Option<&u16> {
        self.name_to_id.get(&name)
    }

    pub fn get_name(&self, id: u16) -> Option<&String> {
        self.id_to_name.get(&id)
    }
}

pub type LockstepClientCommands = HashMap<ClientId, Option<Vec<LockstepCommand>>>;

/// The client sends commands to the server and they get stored in this buffer
/// based on the tick they received from the client, i.e. the tick when issued.
/// This is only used on the server.  It's sole purpose is to track who is still 
/// sending data currently so that we can detect disconnects.
#[derive(Resource, Default, Deref, DerefMut, Debug)]
pub(crate) struct LockstepGameCommandsReceived(HashMap<SimTick, LockstepClientCommands>);

/// This wraps the same as LockstepGameCommandsReceived. The difference is that
/// this is used on both clients and the server, and the tick keys have been delayed.
/// The server broadcasts commands to clients and they get stored in this buffer.
/// Inputs have client delays added to the tick to account for the ping of each client.
/// Users should implement systems in FixedUpdate to handle these commands.
#[derive(Resource, Default, Deref, DerefMut, Debug)]
pub struct LockstepGameCommandBuffer(HashMap<SimTick, LockstepClientCommands>);

/// An event type for clients to send their commands for their current tick to the server
#[derive(Event, Serialize, Deserialize, Default, Debug)]
pub struct ClientSendCommands {
    pub commands: Option<Vec<LockstepCommand>>,
    pub issued_tick: SimTick,
}

/// An event type for the server to broadcast received commands to clients
#[derive(Event, Serialize, Deserialize, Default)]
struct ServerSendCommands {
    commands: Option<Vec<LockstepCommand>>,
    execute_tick: SimTick,
    client_id: ClientId,
}

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
/// At the beginning of each tick send an empty command queue
/// just to let the server know we are still in the game
fn send_empty_commands_to_server_on_tick(
    mut ticks: EventReader<SimulationTickEvent>,
    mut commands: Commands,
    sim_tick: Res<SimulationTick>,
    local_client: Query<&LocalClient>,
) {
    // Dont send commands if in dedicated server mode
    if local_client.get_single().is_err() { return }

    for _tick in ticks.read() {
        trace!("tick changed to {}, sending comamnds", **sim_tick);
        commands.client_trigger(ClientSendCommands {
            issued_tick: **sim_tick,
            ..default()
        });
    }
}

/// When the server receives commmands from a client it should
///  - store the commands in the command history
///  - broadcast them to all other clients
fn receive_commands_server(
    trigger: Trigger<FromClient<ClientSendCommands>>,
    mut history: ResMut<LockstepGameCommandsReceived>,
    clients: Query<&NetworkId>,
    settings: Res<SimulationSettings>,
    stats: Query<&NetworkStats>,
    mut commands: Commands,
) { 
    // In host server mode, the server can send events to itself
    // Server sent events use Entity::PLACEHOLDER
    // Instead I have set Host to have its own entity which has NetworkId=1
    let client_id = clients.get(trigger.client_entity).map_or(1, |id| id.get());
    trace!("server received commands from client {} for tick {}", client_id, trigger.event().issued_tick);

    let tick = trigger.event().issued_tick;
    history.entry(tick)
        .or_insert_with(LockstepClientCommands::new)
        .entry(client_id)
        .insert(trigger.event().commands.clone());

    // Broadcast to all clients with a tick delay
    // Input tick delay depends on ping, for host server default to 3 ticks for now
    let tick_delay = stats
        .get(trigger.client_entity)
        .map_or(3, |s| (s.rtt / 2.0).ceil() as SimTick);
    let execution_tick = tick + tick_delay + settings.base_input_tick_delay as SimTick;
    trace!("sending commands for execution tick {} for client {}", execution_tick, client_id);
    commands.server_trigger(ToClients {
        mode: SendMode::Broadcast,
        event: ServerSendCommands {
            execute_tick: execution_tick,
            commands: trigger.event().commands.clone(),
            client_id: client_id,
        }
    });
}

///  Store the commands from the server in a command buffer to be executed in future ticks
fn receive_commands_client(
    trigger: Trigger<ServerSendCommands>,
    mut game_commands: ResMut<LockstepGameCommandBuffer>,
) { 
    //trace!("local client received commands for execution tick {} from {}", trigger.execute_tick, trigger.client_id);
    game_commands.0
        .entry(trigger.execute_tick)
        .or_insert(LockstepClientCommands::default())
        .entry(trigger.client_id)
        .insert(trigger.commands.clone()); 
}
