use thiserror::Error;

use crate::action::AgentAction;

#[derive(Clone, Debug, PartialEq)]
pub enum Skill {
    YoutubeSearch { query: String },
    GenericClick { target: String },
}

#[derive(Debug, Error, PartialEq)]
pub enum SkillError {
    #[error("unsupported task in MVP")]
    UnsupportedTask,
}

impl Skill {
    pub fn parse(goal: &str) -> Result<Self, SkillError> {
        let trimmed = goal.trim();
        let lower = trimmed.to_ascii_lowercase();
        let prefix = "search youtube for ";
        if lower.starts_with(prefix) {
            let query = trimmed[prefix.len()..].trim();
            if !query.is_empty() {
                return Ok(Self::YoutubeSearch {
                    query: query.into(),
                });
            }
        }
        if lower.starts_with("click on ") {
            let target = trimmed[9..].trim();
            if !target.is_empty() {
                return Ok(Self::GenericClick {
                    target: target.into(),
                });
            }
        }
        if lower.starts_with("click ") {
            let target = trimmed[6..].trim();
            if !target.is_empty() {
                return Ok(Self::GenericClick {
                    target: target.into(),
                });
            }
        }
        Err(SkillError::UnsupportedTask)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::YoutubeSearch { .. } => "youtube_search",
            Self::GenericClick { .. } => "generic_click",
        }
    }

    pub fn candidates(&self, history: &[AgentAction]) -> Vec<AgentAction> {
        match self {
            Self::YoutubeSearch { query } => {
                if !history
                    .iter()
                    .any(|a| matches!(a, AgentAction::ClickTarget { .. }))
                {
                    vec![
                        AgentAction::ClickTarget {
                            target: "the search input inside the YouTube webpage header, at the top center. Ignore terminal windows, command text, tabs, and title bars.".into(),
                        },
                        AgentAction::Done,
                        AgentAction::Abort {
                            reason: "unsafe to continue".into(),
                        },
                    ]
                } else if !history
                    .iter()
                    .any(|a| matches!(a, AgentAction::TypeText { .. }))
                {
                    vec![
                        AgentAction::TypeText {
                            text: query.clone(),
                        },
                        AgentAction::Done,
                        AgentAction::Abort {
                            reason: "unsafe to continue".into(),
                        },
                    ]
                } else if !history.iter().any(|a| matches!(a, AgentAction::PressEnter)) {
                    vec![
                        AgentAction::PressEnter,
                        AgentAction::Done,
                        AgentAction::Abort {
                            reason: "unsafe to continue".into(),
                        },
                    ]
                } else {
                    vec![AgentAction::Done]
                }
            }
            Self::GenericClick { target } => {
                if !history
                    .iter()
                    .any(|a| matches!(a, AgentAction::ClickTarget { .. }))
                {
                    vec![
                        AgentAction::ClickTarget {
                            target: target.clone(),
                        },
                        AgentAction::Abort {
                            reason: "unsafe to continue".into(),
                        },
                    ]
                } else {
                    vec![AgentAction::Done]
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_youtube_search() {
        assert_eq!(
            Skill::parse("Search YouTube for rust programming"),
            Ok(Skill::YoutubeSearch {
                query: "rust programming".into()
            })
        );
    }

    #[test]
    fn parses_generic_click() {
        assert_eq!(
            Skill::parse("Click the Discord icon"),
            Ok(Skill::GenericClick {
                target: "the Discord icon".into()
            })
        );
        assert_eq!(
            Skill::parse("Click on Add Friend"),
            Ok(Skill::GenericClick {
                target: "Add Friend".into()
            })
        );
    }

    #[test]
    fn rejects_unknown_task() {
        assert_eq!(
            Skill::parse("Open my calendar"),
            Err(SkillError::UnsupportedTask)
        );
        assert_eq!(Skill::parse("Click "), Err(SkillError::UnsupportedTask));
    }
}
