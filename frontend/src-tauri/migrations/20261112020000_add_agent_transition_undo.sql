-- The original event is the durable undo receipt; only agent recommendations/reviews receive one.
CREATE TABLE mm_agent_transition_undo (
    event_id INTEGER PRIMARY KEY REFERENCES mm_match_events(id),
    undone_at TEXT
);
