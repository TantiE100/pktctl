use ptmp::{Step, TypeCode, Value};

use super::remote::{Remote, check_args, int_arg, no_args};

const CLASS: &str = "ActivityFile";

#[derive(Debug, Clone)]
pub struct ActivityFixture {
    pub instructions: Vec<String>,
    pub items: (u32, u32),
    pub connectivity: Vec<(String, bool)>,
    pub seconds_left: Option<i32>,
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct Activity {
    fixture: ActivityFixture,
    elapsed_ms: i32,
    tests_run: bool,
    unlocked: bool,
}

impl Activity {
    pub(super) fn new(fixture: ActivityFixture) -> Self {
        Self {
            unlocked: fixture.password.is_none(),
            fixture,
            elapsed_ms: 125_000,
            tests_run: false,
        }
    }
}

pub(super) fn handle(
    activity: Option<&mut Activity>,
    description: &mut String,
    step: &Step,
) -> Result<Value, Remote> {
    match step.method.as_str() {
        "isActivityFile" => return no_args(step, CLASS).map(|()| Value::Bool(activity.is_some())),
        "getNetworkDescription" => {
            return no_args(step, CLASS).map(|()| Value::string(description.as_str()));
        }
        "setNetworkDescription" => {
            check_args(step, CLASS, &[TypeCode::QString])?;
            step.args[0]
                .as_str()
                .unwrap_or_default()
                .clone_into(description);
            return Ok(Value::Void);
        }
        _ => {}
    }
    let activity = activity.ok_or_else(|| Remote::unknown_method("NetworkFile", &step.method))?;
    if let Some(reply) = password_gate(activity, step)? {
        return Ok(reply);
    }
    let (correct, total) = activity.fixture.items;
    let percent = if total == 0 {
        100.0
    } else {
        f64::from(correct) * 100.0 / f64::from(total)
    };
    let getter = |value: Value| no_args(step, CLASS).map(|()| value);
    match step.method.as_str() {
        "getPercentageComplete" | "getPercentageCompleteScore" => getter(Value::Double(percent)),
        "getAssessmentItemsCount" | "getAssessmentScoreCount" => {
            getter(Value::Double(f64::from(total)))
        }
        "getCorrectAssessmentItemsCount" | "getCorrectAssessmentScoreCount" => {
            getter(Value::Double(f64::from(correct)))
        }
        "getInstructionCount" => getter(Value::Int(
            i32::try_from(activity.fixture.instructions.len()).unwrap_or_default(),
        )),
        "getTimeElapsed" => getter(Value::Int(activity.elapsed_ms)),
        "getTimerType" => getter(Value::Int(i32::from(
            activity.fixture.seconds_left.is_some(),
        ))),
        "getCountDownTime" | "getCountDownTimeLeft" => getter(Value::Int(
            activity.fixture.seconds_left.unwrap_or_default() * 1000,
        )),
        "getInstruction" => {
            let index = int_arg(step, CLASS)?;
            usize::try_from(index)
                .ok()
                .and_then(|index| activity.fixture.instructions.get(index))
                .map(Value::string)
                .ok_or_else(|| Remote::missing("instruction page"))
        }
        "runConnectivityTests" => {
            no_args(step, CLASS)?;
            activity.tests_run = true;
            Ok(Value::Void)
        }
        "getConnectivityCount" => getter(Value::Double(if activity.tests_run {
            f64::from(u32::try_from(activity.fixture.connectivity.len()).unwrap_or_default())
        } else {
            0.0
        })),
        "getLastConnectivityTestCorrectCount" => getter(Value::Int(
            i32::try_from(
                activity
                    .fixture
                    .connectivity
                    .iter()
                    .filter(|(_, ok)| *ok)
                    .count(),
            )
            .unwrap_or_default(),
        )),
        "getLastConnectivityTestResultAt" => {
            let index = int_arg(step, CLASS)?;
            usize::try_from(index)
                .ok()
                .and_then(|index| activity.fixture.connectivity.get(index))
                .map(|(line, _)| Value::string(line))
                .ok_or_else(|| Remote::missing("connectivity result"))
        }
        "resetActivity" => {
            no_args(step, CLASS)?;
            activity.elapsed_ms = 0;
            activity.tests_run = false;
            Ok(Value::Void)
        }
        other => Err(Remote::unknown_method(CLASS, other)),
    }
}

fn password_gate(activity: &mut Activity, step: &Step) -> Result<Option<Value>, Remote> {
    match step.method.as_str() {
        "isPasswordConfirmed" => {
            no_args(step, CLASS).map(|()| Some(Value::Bool(activity.unlocked)))
        }
        "confirmPassword" => {
            check_args(step, CLASS, &[TypeCode::QString])?;
            let given = step.args[0].as_str().unwrap_or_default();
            activity.unlocked = activity
                .fixture
                .password
                .as_deref()
                .is_none_or(|password| password == given);
            Ok(Some(Value::Bool(activity.unlocked)))
        }
        "getInstructionCount" | "getInstruction" => Ok(None),
        method if !activity.unlocked => Err(Remote {
            class: CLASS.into(),
            message: format!(
                "Activity file requires password, call ipc.appWindow().getActiveFile().confirmPassword(passwordString) \"{method}\""
            ),
        }),
        _ => Ok(None),
    }
}
