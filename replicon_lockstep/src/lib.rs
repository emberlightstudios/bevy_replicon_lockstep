use bevy::prelude::*;

mod simulation;
mod connections;
pub mod commands;

use commands::LockstepCommandsPlugin;
use connections::LockstepConnectionsPlugin;
use simulation::LockstepSimulationPlugin;
use prelude::*;

pub mod prelude {
    pub use crate::RepliconLockstepPlugin;
    pub use crate::simulation::{
        SimulationSettings,
        SimulationState,
        SimTick,
        SimulationTick,
        SimulationTickUpdate,
        SimulationId,
        SimulationIdEntityMap,
    };
    pub use crate::connections::{
        LocalClient,
        ClientId,
        ClientReconnect,
        ClientDisconnect,
        ClientReadyEvent,
        ServerMode,
        ConnectionSettings,
    };
    pub use crate::commands::{
        ClientSendCommands,
        LockstepGameCommandBuffer,
        LockstepClientCommands,
    };
}

#[derive(Default)]
pub struct RepliconLockstepPlugin {
    pub simulation: SimulationSettings,
    pub server: ConnectionSettings,
}

impl Plugin for RepliconLockstepPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(self.simulation.clone())
            .insert_resource(self.server.clone())
            .add_plugins((
                LockstepConnectionsPlugin,
                LockstepSimulationPlugin,
                LockstepCommandsPlugin,
            ))
            .insert_resource(Time::<Fixed>::from_duration(self.simulation.tick_timestep));
    }
}