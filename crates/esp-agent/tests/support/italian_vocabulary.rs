// ANCHOR: italian
static ITALIAN: Vocabulary<Action> = vocabulary! {
    Action::TurnOn => explicit ["accendi {target}"], previous ["accendila", "accendilo"];
    Action::TurnOff => explicit ["spegni {target}"],
        previous ["spegnila", "spegnilo", "basta così"];
    Action::SetBrightness { level: Level } => explicit ["imposta {target} al {level}%"],
        previous ["impostala al {level}%"];
};
// ANCHOR_END: italian
