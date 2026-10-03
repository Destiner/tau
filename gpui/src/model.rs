use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Props {
    pub label: String,
    pub value: String,
    pub variant: String,
    pub size: String,
    pub placeholder: String,
    pub disabled: bool,
    pub readonly: bool,
    pub error: bool,
    pub width: f32,
    pub rows: usize,
    pub monospaced: bool,
}
impl Props {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.width.is_finite() && self.width > 0.,
            "width must be positive and finite"
        );
        anyhow::ensure!(self.rows > 0, "rows must be positive");
        Ok(())
    }
}

impl Default for Props {
    fn default() -> Self {
        Self {
            label: "Continue".into(),
            value: String::new(),
            variant: String::new(),
            size: "sm".into(),
            placeholder: String::new(),
            disabled: false,
            readonly: false,
            error: false,
            width: 360.,
            rows: 4,
            monospaced: false,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub component: String,
    #[serde(default)]
    pub props: Props,
    pub steps: Vec<Step>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum Step {
    Focus,
    Blur,
    Click,
    Hover,
    MouseDown,
    MouseUp,
    Pointer {
        phase: PointerPhase,
        x: f32,
        y: f32,
    },
    Key {
        key: String,
    },
    Type {
        text: String,
    },
    Select {
        start: usize,
        end: usize,
    },
    Props {
        props: serde_json::Value,
    },
    Checkpoint {
        name: String,
        expected: serde_json::Value,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PointerPhase {
    Move,
    Down,
    Up,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub value: String,
    pub selection: Option<[usize; 2]>,
    pub focused: bool,
    pub disabled: bool,
    pub readonly: bool,
    pub clicks: usize,
    pub changes: usize,
}
