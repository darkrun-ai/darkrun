# Review and feedback

darkrun keeps a human at the checkpoints, not in the weeds. When a station
produces a **Unit** you can open it, read its output, and respond — without
stopping the rest of the line.

## The review session

Open a Run's **Review** screen and you see the current station, its phase, and
every Unit it has produced. Each Unit shows its type, its status, and which
**Pass** it is on. The desktop app and the website render the same session
payload over the local engine's WebSocket feed.

## Leaving feedback

Feedback is anchored. You can pin a comment to a Unit as a whole, or inline to a
specific span of its output. Each comment carries a **severity** and a
**status** — open, resolved, or closed — so a station knows what is still
outstanding before it can lock.

## Decisions

A review ends in a decision: **approve** advances the checkpoint, **request
changes** sends the Unit back for another Pass through the fix-workers. The
manager records the decision and the iteration result so the next pass starts
with full context.
