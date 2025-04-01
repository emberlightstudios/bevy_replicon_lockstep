use bevy::prelude::*;
use avian3d::prelude::*;

mod simulation;
mod connections;
mod commands;

use connections::LockstepConnectionsPlugin;
use simulation::{
    LockstepSimulationPlugin,
    SimulationSettings,
};
use commands::{
    CommandTypeRegistry,
    LockstepCommandsPlugin,
};

pub mod prelude {
    pub use crate::RepliconLockstepPlugin;
    pub use crate::simulation::{
        SimulationSettings,
        SimulationState,
        SimulationTick,
        SimTick,
    };
    pub use crate::connections::{
        LocalClient,
        ClientId,
        ClientReadyEvent,
        ClientReady,
    };
    pub use crate::commands::{
        ClientSendCommands,
        LockstepGameCommandBuffer,
        LockstepCommand,
        CommandTypeRegistry,
        LockstepClientCommands,
    };
}

#[derive(Default)]
pub struct RepliconLockstepPlugin {
    pub simulation: SimulationSettings,
    pub commands: Vec<String>,
}

impl Plugin for RepliconLockstepPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(self.simulation.clone())
            .insert_resource(CommandTypeRegistry::new(self.commands.clone()))
            .add_plugins((
                LockstepConnectionsPlugin,
                LockstepSimulationPlugin,
                LockstepCommandsPlugin,
                PhysicsPlugins::default(),
            ))
            .insert_resource(Time::<Fixed>::from_duration(self.simulation.tick_timestep));
    }
}