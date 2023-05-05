use crate::prelude::*;
use log::*;
use std::fmt::Debug;
use std::sync::Arc;
use yarn_slinger_core::prelude::*;

/// Co-ordinates the execution of Yarn programs.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Dialogue {
    /// Gets the [`Library`] that this Dialogue uses to locate functions.
    ///
    /// When the Dialogue is constructed, the Library is initialized with
    /// the built-in operators like `+`, `-`, and so on.
    pub library: Library,

    /// The object that provides access to storing and retrieving the values of variables.
    pub variable_storage: Arc<dyn VariableStorage + Send + Sync>,

    /// Invoked when the Dialogue needs to report debugging information.
    log_debug_message: Logger,

    /// Invoked when the Dialogue needs to report an error.
    log_error_message: Logger,

    /// The [`Dialogue`]'s locale, as an IETF BCP 47 code.
    ///
    /// This code is used to determine how the `plural` and `ordinal`
    /// markers determine the plural class of numbers.
    ///
    /// For example, the code "en-US" represents the English language as
    /// used in the United States.
    pub language_code: Option<String>,

    vm: VirtualMachine,
}

impl Default for Dialogue {
    fn default() -> Self {
        let variable_storage: Arc<dyn VariableStorage + Send + Sync> =
            Arc::new(MemoryVariableStore::default());

        let library = {
            let storage_one = variable_storage.clone();
            let storage_two = variable_storage.clone();
            Library::standard_library()
                .with_function("visited", move |node: String| -> bool {
                    is_node_visited(storage_one.as_ref(), &node)
                })
                .with_function("visited_count", move |node: String| -> f32 {
                    get_node_visit_count(storage_two.as_ref(), &node)
                })
        };

        Self {
            library,
            variable_storage,
            log_debug_message: Logger(Box::new(|msg| debug!("{msg}"))),
            log_error_message: Logger(Box::new(|msg| error!("{msg}"))),
            language_code: Default::default(),
            vm: Default::default(),
        }
    }
}

impl Dialogue {
    pub const DEFAULT_START_NODE_NAME: &'static str = "Start";

    /// Initializes a new instance of the [`Dialogue`] class.
    pub fn with_variable_storage(
        mut self,
        variable_storage: impl VariableStorage + 'static + Send + Sync,
    ) -> Self {
        self.variable_storage = Arc::new(variable_storage);
        self
    }

    pub fn with_log_debug_message(
        mut self,
        logger: impl Fn(String) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.log_debug_message = Logger(Box::new(logger));
        self
    }

    pub fn with_log_error_message(
        mut self,
        logger: impl Fn(String) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.log_error_message = Logger(Box::new(logger));
        self
    }

    pub fn with_line_handler(
        mut self,
        line_handler: impl Fn(Line) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.vm.line_handler = LineHandler(Box::new(line_handler));
        self
    }

    /// The [`OptionsHandler`] that is called when a set of options are ready to be shown to the user.
    ///
    /// The Options Handler delivers a [`Vec`] of [`DialogueOption`] to the game.
    /// Before [`Dialogue::continue_`] can be called to resume execution,
    /// [`Dialogue::set_selected_option`] must be called to indicate which
    /// [`DialogueOption`] was selected by the user. If [`Dialogue::set_selected_option`] is not called, a panic occurs.
    pub fn with_options_handler(
        mut self,
        options_handler: impl Fn(Vec<DialogueOption>) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.vm.options_handler = OptionsHandler(Box::new(options_handler));
        self
    }

    /// The [`CommandHandler`] that is called when a command is to be delivered to the game.
    pub fn with_command_handler(
        mut self,
        command_handler: impl Fn(Command) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.vm.command_handler = CommandHandler(Box::new(command_handler));
        self
    }

    /// The [`NodeCompleteHandler`] that is called when a node is complete.
    pub fn with_node_complete_handler(
        mut self,
        node_complete_handler: impl Fn(NodeName) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.vm.node_complete_handler = NodeCompleteHandler(Box::new(node_complete_handler));
        self
    }

    /// The [`NodeStartHandler`] that is called when a node is started.
    pub fn with_node_start_handler(
        mut self,
        node_start_handler: impl Fn(NodeName) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.vm.node_start_handler = NodeStartHandler(Box::new(node_start_handler));
        self
    }

    /// The [`DialogueCompleteHandler`] that is called when the Dialogue reaches its end.
    pub fn with_dialogue_complete_handler(
        mut self,
        dialogue_complete_handler: impl Fn() + Clone + 'static + Send + Sync,
    ) -> Self {
        self.vm.dialogue_complete_handler =
            DialogueCompleteHandler(Box::new(dialogue_complete_handler));
        self
    }

    /// The [`PrepareForLinesHandler`] that is called when the dialogue anticipates delivering some lines.
    pub fn with_prepare_for_lines_handler(
        mut self,
        prepare_for_lines_handler: impl Fn(Vec<LineId>) + Clone + 'static + Send + Sync,
    ) -> Self {
        self.vm.prepare_for_lines_handler =
            PrepareForLinesHandler(Box::new(prepare_for_lines_handler));
        self
    }

    pub fn with_language_code(self, language_code: impl Into<String>) -> Self {
        Self {
            language_code: Some(language_code.into()),
            ..self
        }
    }

    /// Gets a value indicating whether the Dialogue is currently executing Yarn instructions.
    pub fn is_active(&self) -> bool {
        self.vm.execution_state() != ExecutionState::Stopped
    }

    pub fn with_new_program(mut self, program: Program) -> Self {
        self.set_program(program);
        self
    }

    pub fn with_additional_program(mut self, program: Program) -> Self {
        self.add_program(program);
        self
    }

    pub fn set_program(&mut self, program: Program) -> &mut Self {
        self.vm.program = Some(program);
        self.vm.reset_state();
        self
    }

    pub fn add_program(&mut self, program: Program) -> &mut Self {
        if let Some(existing_program) = &mut self.vm.program {
            *existing_program = Program::combine(vec![existing_program.clone(), program]).unwrap();
        } else {
            self.set_program(program);
        }
        self
    }

    /// Prepares the [`Dialogue`] that the user intends to start running a node.
    ///
    /// After this method is called, you call [`Dialogue::continue_`] to start executing it.
    ///
    /// If [`Dialogue::prepare_for_lines_handler`] has been set, it may be called when this method is invoked,
    /// as the Dialogue determines which lines may be delivered during the `start_node` node's execution.
    ///
    /// ## Panics
    ///
    /// Panics if no node named `start_node` has been loaded.
    pub fn set_node(&mut self, start_node: &str) {
        self.vm.set_node(start_node);
    }

    pub fn set_start_node(&mut self) {
        self.set_node(Self::DEFAULT_START_NODE_NAME);
    }

    /// Signals to the [`Dialogue`] that the user has selected a specified [`DialogueOption`].
    ///
    /// After the Dialogue delivers an [`OptionSet`], this method must be called before [`Dialogue::continue_`] is called.
    ///
    /// The ID number that should be passed as the parameter to this method should be the [`DialogueOption::Id`]
    /// field in the [`DialogueOption`] that represents the user's selection.
    ///
    /// ## Panics
    /// - If the Dialogue is not expecting an option to be selected.
    /// - If the option ID is not found in the current [`OptionSet`].
    ///
    /// ## See Also
    /// - [`Dialogue::continue_`]
    /// - [`OptionsHandler`]
    /// - [`OptionSet`]
    pub fn set_selected_option(&mut self, selected_option_id: OptionId) {
        self.vm.set_selected_option(selected_option_id);
    }

    /// Starts, or continues, execution of the current program.
    ///
    /// This method repeatedly executes instructions until one of the following conditions is encountered:
    /// - The [`LineHandler`] or [`CommandHandler`] is called. After calling either of these handlers, the Dialogue will wait until [`Dialogue::continue_`] is called.
    /// - The [`OptionsHandler`] is called. When this occurs, the Dialogue is waiting for the user to specify which of the options has been selected,
    /// and [`Dialogue::set_selected_option`] must be called before [`Dialogue::continue_`] is called.
    /// - The program reaches its end. When this occurs, [`Dialogue::set_node`] must be called before [`Dialogue::continue_`] is called again.
    /// - An error occurs while executing the program
    ///
    /// This method has no effect if it is called while the [`Dialogue`] is currently in the process of executing instructions.
    ///
    /// ## See Also
    /// - [`LineHandler`]
    /// - [`OptionsHandler`]
    /// - [`CommandHandler`]
    /// - [`NodeCompleteHandler`]
    /// - [`DialogueCompleteHandler`]
    ///
    /// ## Implementation Notes
    ///
    /// The original states that the [`LineHandler`] and [`CommandHandler`] may call [`Dialogue::continue_`]. Because of the borrow checker,
    /// this is action is very unidiomatic and impossible to do without introducing a lot of interior mutability all along the API.
    /// For this reason, we disallow mutating the [`Dialogue`] within any handler.
    pub fn continue_(&mut self) {
        // Cannot 'continue' an already running VM.
        if self.vm.execution_state() != ExecutionState::Running {
            self.vm.continue_();
        }
    }
}

fn is_node_visited(variable_storage: &dyn VariableStorage, node_name: &str) -> bool {
    if let Some(YarnValue::Number(count)) = variable_storage.get(node_name) {
        count > 0.0
    } else {
        false
    }
}

fn get_node_visit_count(variable_storage: &dyn VariableStorage, node_name: &str) -> f32 {
    if let Some(YarnValue::Number(count)) = variable_storage.get(node_name) {
        count
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;

    #[test]
    fn can_set_handler() {
        let _dialogue = Dialogue::default()
            .with_log_debug_message(|_| {})
            .with_options_handler(|_| {});
    }

    #[test]
    fn is_send_sync() {
        let dialogue = Dialogue::default();
        accept_send_sync(dialogue);
    }

    fn accept_send_sync(_: impl Send + Sync) {}
}
