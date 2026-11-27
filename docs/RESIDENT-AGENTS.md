# Agents résidents et multi-instances — conception

> **Étapes 1 à 3 construites** (état par agent, bail inter-process, agents résidents : serveur
> propre, file, `ask` / `status` / `stop`, annonce ; instances numérotées `swarm-control-N` et
> routage des runs vers l'hôte — voir §3 et §6). **Le reste est une proposition, non
> construite** (exécutable par agent, pairs automatiques). Décidée le 2026-11-26 : les agents
> sont **résidents**, et l'architecture accepte **plusieurs instances**.
> Les formats d'annonce (`litemcp-registry`) sont ceux de LiteMCP. Construits : `ask`, `status`,
> `stop`, `agent_start`, `agent_stop`, `--instance`. **À construire** : `peers`, le champ `executable`.

## 1. Trois objets

| Objet | Ce que c'est | Où |
|---|---|---|
| **Instance** | un process `swarm-control` : un serveur d'administration (10 outils, dont `agent_start`, `agent_stop`) | nom `swarm-control-N`, un port |
| **Agent résident** | un agent *chargé en mémoire* : son propre serveur MCP, sa file de demandes, son fil de travail | nom `swarm-<agent>`, un port |
| **Pair** | un autre agent résident, retrouvé par le registre LiteMCP | `%TEMP%/litemcp-registry/swarm-*.json` |

Un exécutable = le même binaire sous un autre nom : `alice.exe` lancé sans argument sert l'agent
`alice` (le nom du fichier sert d'identité) ; `swarm-control` seul peut en héberger plusieurs.

```
client MCP ─▶ swarm-control-1 (admin :23080)             swarm-control-2 (admin :23081)
                 héberge  ┌ swarm-alice  :23082              héberge  └ swarm-carol :23084
                          └ swarm-bob    :23083
  le CLI de alice (claude -p) ──MCP──▶ swarm-bob / swarm-carol    (adresses lues dans le registre)
```

## 2. Ce qui vit en mémoire, par agent résident

```rust
struct AgentHost {            // une entrée par agent résident, dans l'instance
    name: String, runner: Runner,
    inbox: Queue<Turn>,       // demandes en attente (FIFO) — remplace AGENT_BUSY par une file
    worker: JoinHandle<()>,   // UN fil : exécute les tours l'un après l'autre
    server: BoundServer,      // son propre listener LiteMCP, annoncé sous « swarm-<name> »
    current: Option<LiveRun>, // run en cours : PID, sortie (256 Kio), drapeau kill
}
```

- **Rien d'autre n'est gardé en RAM** : la conversation reste dans le CLI (`--resume <id>`), la
  définition sur disque. Un agent au repos = 2 fils bloqués + sa file. **Mesuré** (build release,
  Windows 11, *working set*, une exécution) : 5,1 Mio sans agent → 7,0 Mio avec 24 agents résidents,
  soit **≈ 80 Kio et 2 fils par agent** ; après une demande à chacun : ≈ 9 Mio, 78 fils (un fil de
  connexion par client resté connecté). Les 20 premiers agents prennent la tranche 23080..=23099,
  les suivants des ports éphémères (vérifié : 4 agents hors tranche).
- Le CLI est **toujours lancé une fois par tour** (ces CLI sont à usage unique) ; le résident
  porte la file, l'état et l'identité, pas le modèle.
- Un seul tour à la fois par agent : la file garantit qu'une session n'est jamais reprise deux fois.

## 3. Plusieurs instances : les règles

1. **Nom** *(✅ construit)* : `swarm-control-N`, `N` = premier numéro libre (ou `--instance N`,
   refusé s'il est pris). Le numéro est réservé par un bail (`%TEMP%/swarm-control-instances/N.json`,
   même mécanique que les autres baux, battement 30 s) ; l'instance s'annonce sous ce nom
   (`announce()` + `anti_zombie()` : les annonces d'instances mortes sont retirées). LiteMCP écrase
   l'annonce d'un même nom : sans numéro, la 2ᵉ instance masquerait la 1ʳᵉ.
2. **Propriété d'un agent** *(✅ construit : bail de **run** pris par chaque tour, bail
   d'**hébergement** pris par chaque agent résident)* : une seule instance fait tourner / héberge un agent. Bail créé de façon exclusive
   (`create_new`) : `<ws>/bridge/owners/<runner>-<agent>.json` (`{instance, pid, run_id, ts}`),
   renouvelé toutes les 30 s, périmé après 2 min sans battement. Verrou périmé = PID mort (test déjà utilisé par le
   balayeur : `process::pid_alive`) ou URL qui ne répond plus.
3. **État par agent, plus de fichier global partagé** *(✅ construit — étape 1)* : `agents.json` et `process-registry.json`
   sont relus et réécrits en entier ; nos verrous (`static Mutex`) ne protègent qu'un process. On
   passe à `<ws>/bridge/state/<runner>-<agent>.json` (session + dernier run), écrit seulement par
   le propriétaire. Les anciens fichiers restent lus (compatibilité OverMind TS).
4. **Routage** *(✅ construit)* : `run_agent("alice")` (et chaque tâche de `run_agents_parallel`)
   d'un agent qui a un hôte vivant est **transmis** à cet hôte par `ask` (un `POST /mcp` en `std`,
   module `peers`), même si l'hôte est l'instance qui reçoit l'appel : la file reste l'unique voie.
   La réponse garde la forme de `run_agent` plus `routedTo {instance, url}`. **Reste local, à la
   demande**, exactement comme avant : agent sans hôte, hôte injoignable (connexion refusée avant
   tout envoi), `background:true`, ou run imposant `sessionId`, `path`, `provider` ou
   `autoResume:false` (`ask` reprend toujours la session de l'agent, dans le workspace de l'hôte).
5. **Découverte** : liste des `swarm-*.json` du registre (champs LiteMCP : `name, port, pid, instance,
   exe, args, cwd, ts`) ; le balayeur anti-zombie de LiteMCP retire les annonces mortes.

## 4. Payloads

### `create_agent` (instance 1, admin)
```json
{"name":"create_agent","arguments":{"name":"alice","runner":"claude","prompt":"Tu es Alice.","resident":true}}
```
```json
{"created":"alice","runner":"claude",
 "executable":"C:\\Users\\Deamon\\.swarm-control\\bin\\alice.exe",          // étape 4 : pas encore rendu
 "resident":{"instance":"swarm-control-1","name":"swarm-alice","url":"http://127.0.0.1:23082/mcp"}}
```

### Annonce LiteMCP de l'agent (`%TEMP%/litemcp-registry/swarm-alice.json`, format LiteMCP)
```json
{"name":"swarm-alice","version":"0.1.0","addr":"127.0.0.1:23082","port":23082,
 "instance":8123456789,"pid":21132,"exe":"…\\bin\\alice.exe","args":[],"cwd":"…","ts":1795738511}
```
⚠ LiteMCP publie `args` : aucun secret en argv (jetons par l'environnement, comme aujourd'hui).

### Outils du serveur de l'agent (`tools/list` sur `swarm-alice`)
| Outil | Rôle |
|---|---|
| `ask` | met un tour dans la file, rend la réponse |
| `status` | file, tour en cours, session, PID |
| `stop` | tue le tour en cours (arbre de processus) |
| `peers` | les autres agents résidents vus dans le registre |

### `ask` (de bob vers alice)
```json
{"name":"ask","arguments":{"prompt":"Propose un plan d'API","from":"bob","via":["bob"],"timeoutMs":600000}}
```
```json
{"agent":"alice","reply":"…","sessionId":"…","queuedMs":0,"durationMs":9120,"via":["bob","alice"]}
```
Refus d'un cycle (`bob → alice → bob`) :
```json
{"code":"invalid_input","message":"CYCLE: bob est déjà dans la chaîne [bob, alice]"}
```

### Les pairs, vus par le modèle
Avant chaque tour, l'agent regénère son fichier MCP avec les pairs découverts :
```json
{"mcpServers":{"swarm-bob":{"type":"http","url":"http://127.0.0.1:23083/mcp"},
               "swarm-carol":{"type":"http","url":"http://127.0.0.1:23084/mcp"}}}
```
Claude le reçoit par `--mcp-config` (déjà le cas pour ses serveurs) ; Hermes par `mcp_servers` de son
`config.yaml` (déjà écrit par `agents/hermes/mcp.rs`). Son modèle appelle alors `swarm-bob.ask(…)`
comme n'importe quel outil.

## 5. Risques et questions ouvertes

| Risque | Parade prévue |
|---|---|
| **Interblocage** : A demande à B pendant que B demande à A (un seul fil par agent) | chaîne `via` + refus des cycles + `timeoutMs` ; à tester |
| **Durée** : un tour dure des minutes, le client MCP du CLI peut couper avant | `ask` synchrone avec délai, plus une variante asynchrone (`taskId`) — à vérifier sur les CLI réels |
| **Sécurité** : agents en `--dangerously-skip-permissions` qui se pilotent entre eux, sans authentification | loopback seulement ; signature de requêtes LiteMCP (`auth-sign`) avant toute ouverture réseau |
| **Mise à jour du binaire** : un `alice.exe` copié ne suit pas `swarm-control` | liens physiques ou recréation (`swarm-control upgrade`) |
| **Ports** : tranche par défaut = 20 | repli éphémère de LiteMCP ; l'annonce publie le port réel |
| Runners sans configuration MCP injectable (opencode, Kilo, Cline, OpenClaw) | pairs joignables par `ask` côté orchestrateur ; injection au cas par cas, à vérifier runner par runner |

## 6. Étapes testables (de bout en bout, faux CLI d'abord)

1. ✅ **État par agent** + verrou de propriété (sans résidence encore) ; `agents.json` reste lu en
   repli. Construit et testé : 16 acquéreurs simultanés → un seul gagnant (bail frais ou laissé
   par un mort), 2 instances sur un même workspace partagent la session et s'excluent mutuellement.
2. ✅ **Agent résident** : serveur propre, file, `ask` / `status` / `stop`, annonce `swarm-<agent>`,
   `agent_start` / `agent_stop` / `create_agent resident:true` / `--resident`. Construit et testé
   de bout en bout : file FIFO sur une même session, `stop` d'un tour, annulation d'une demande qui
   expire avant de démarrer, attente d'un run « à la demande », exclusivité entre instances,
   passage de l'hébergement d'une instance à l'autre, cycles refusés.
3. ✅ **Plusieurs instances** : numérotation (`swarm-control-N`, `--instance`), annonce, routage vers
   l'hôte, mode à la demande en repli. Testé de bout en bout : numéros distincts et annonces,
   numéro explicite déjà pris refusé, `run_agent` reçu par B exécuté par l'hôte A avec session
   partagée, cas qui restent locaux, tâches parallèles routées.
4. **Exécutable par agent** : création du binaire nommé, identité lue dans le nom du fichier.
5. **Pairs** : découverte, injection dans la configuration MCP, anti-cycle.
