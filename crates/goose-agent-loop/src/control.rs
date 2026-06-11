use tokio_util::sync::CancellationToken;

pub const DEFAULT_AGENT_LOOP_MAX_TURNS: u32 = 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentLoopTurnLimit {
    explicit_max_turns: Option<u32>,
    configured_max_turns: Option<u32>,
    default_max_turns: u32,
}

impl Default for AgentLoopTurnLimit {
    fn default() -> Self {
        Self {
            explicit_max_turns: None,
            configured_max_turns: None,
            default_max_turns: DEFAULT_AGENT_LOOP_MAX_TURNS,
        }
    }
}

impl AgentLoopTurnLimit {
    pub fn new(explicit_max_turns: Option<u32>) -> Self {
        Self {
            explicit_max_turns,
            ..Self::default()
        }
    }

    pub fn with_configured_max_turns(mut self, configured_max_turns: Option<u32>) -> Self {
        self.configured_max_turns = configured_max_turns;
        self
    }

    pub fn with_default_max_turns(mut self, default_max_turns: u32) -> Self {
        self.default_max_turns = default_max_turns;
        self
    }

    pub fn resolve(self) -> u32 {
        self.explicit_max_turns
            .or(self.configured_max_turns)
            .unwrap_or(self.default_max_turns)
    }
}

#[derive(Clone, Debug)]
pub struct AgentLoopControl {
    max_turns: u32,
    turns_taken: u32,
    cancellation_token: Option<CancellationToken>,
}

impl AgentLoopControl {
    pub fn new(max_turns: u32, cancellation_token: Option<CancellationToken>) -> Self {
        Self {
            max_turns,
            turns_taken: 0,
            cancellation_token,
        }
    }

    pub fn max_turns(&self) -> u32 {
        self.max_turns
    }

    pub fn turns_taken(&self) -> u32 {
        self.turns_taken
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }

    pub fn begin_turn(&mut self, count_turn: bool) -> AgentLoopControlDecision {
        if self.is_cancelled() {
            return AgentLoopControlDecision::Cancelled;
        }

        if count_turn {
            self.turns_taken = self.turns_taken.saturating_add(1);
        }

        if self.turns_taken > self.max_turns {
            AgentLoopControlDecision::MaxTurnsReached
        } else {
            AgentLoopControlDecision::Continue
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentLoopControlDecision {
    Continue,
    Cancelled,
    MaxTurnsReached,
}
