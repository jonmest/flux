# Flux
I am trying to build a distributed load balancer in Rust.

## Notes
Below are some notes regarding my challenges, decisions and more. A bit unstructured, for now!

### challenges
The load balancer needs to be distributed. Hence, all Flux instances need to coordinate on:
1. Which backends are healthy, e.g. if backend A goes down all Flux instances should stop sending traffic to it.
2. Which Flux instances are alive? If Flux instance 1 goes down, the others should be aware.
3. Shared state, backend health statuses, metrics etc.

Raft is likely not a good fit. It provides strong consistency and linearizibility, but is leader-based, requires majority quorum (max 1 failure per 3 nodes) and is quite complex and high latency (maybe a couple hundred milliseconds before re-election in case of failure, details is up for debate but you get the idea).

It is okay with eventual consistency. Flux instances could have slightly different views of backend health. Not being leader based would be more resilient.

That is why I think a gossip protocol would be good here. It is decentralized, eventually consistent, scalable and low overhead.
