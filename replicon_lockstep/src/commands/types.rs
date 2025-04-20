/// Example Command types.  These are just for testing, but if you want 
/// to use them, make sure you register them with app.register_type::<T>()
/// so that reflection wil work


use bevy::prelude::*;
use crate::prelude::SimulationId;


#[derive(Reflect, Clone, Debug, Deref)]
pub struct ServerSpawn(SimulationId);

#[derive(Reflect, Clone, Debug)]
pub struct MoveCommand(pub Vec3);
