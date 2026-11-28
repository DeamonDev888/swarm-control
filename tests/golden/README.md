# Fichiers de référence (contrats publics)

Chaque fichier de ce dossier est la sortie **figée** d'un contrat public de swarm-control,
comparée mot pour mot par les tests de `tests/e2e/contracts.rs` (et quelques tests
unitaires). Un contrat qui change doit mettre à jour sa référence **dans le même commit** :
la revue voit la rupture au lieu de la découvrir en production.

Mettre à jour une référence après un changement voulu :

```sh
SWARM_BLESS=1 cargo test --test e2e contracts
```

Sans `SWARM_BLESS=1`, toute différence fait échouer le test en nommant le fichier à relire.
Les fins de ligne sont ignorées : les références sont les mêmes sur les trois systèmes.

| fichier                   | contrat                                                    |
|---------------------------|------------------------------------------------------------|
| `tools-instance.json`     | `tools/list` de l'instance (noms, descriptions, schémas)    |
| `tools-resident.json`     | `tools/list` d'un agent résident                            |
| `status-keys.txt`         | clés de `status` (résident et `/status` du pont)            |
| `list-agents-keys.txt`    | clés de `list_agents {details:true}`                        |
| `banner.txt`              | bannière de démarrage, instance et mode agent (ports masqués) |
| `help.txt`                | `swarm-control --help` (version masquée)                    |
| `env-vars.txt`            | variables d'environnement lues ou posées (vs README)       |
| `mcp-errors.json`         | erreurs MCP (outil, arguments, agent inconnus)               |
