//! Garde-fou de publication des agents x et google (PLAN §4.2 et §5) : le tour porte
//! `[PUBLICATION] verrouillée` tant que `SWARM_ALLOW_PUBLISH=1` n'est pas posé.
//! Rempli par la tâche RUN.
