# bevy_replicon_lockstep
This repo is an attempt at implementing a lockstep network architecture for bevy based on the bevy_replicon crate.  It needs work and is not ready for production use.  It is my first attempt at writing any netcode, and I am also somewhat inexperienced with rust, so I'm not sure how good it is, but it does appear to work.  See caveats below.

The idea is that netcode can be implemented by only sending input streams between clients rather than other modern netcpde solutions which often involve running multiple different time streams for different entities, predicting the future state, and reconciling divergences with past states from the authoritative server.  Instead if we can ensure all clients can run the game in a deterministic manner, then we can replicates commands only and this can scale extremely well with large numbers of entities, e.g. real-time strategy games.  The downside of this approach is two-fold:

1. There is an inherent lag based on the round trip time of a cient.  Since we are not doing client-side prediction it can be noticeable and will not be appropriate for fast paced games, such as first person shooters.
2. Clients must be able to simulate the simulation deterministically so that each client's game state stays in sync with all the others.  This is not always easy to do.

Unlike tradition p2p deterministic lockstep solutions, this crate uses the client-server model.  This has an advantage in that the input delay can be set on a per client basis, rather than everyone's experience being slowed down to match the client with the highest ping.

My original motivation was to try to use avian3d as a physics layer inside the simulation.  While I am able to get forces to work across clients, unfortunately this appears to not be deterministic

