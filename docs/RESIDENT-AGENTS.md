# Agents résidents et multi-instances — conception

> **Étapes 1 à 3 construites** (état par agent, bail inter-process, agents résidents : serveur
> propre, file, `ask` / `status` / `stop`, annonce ; instances numérotées `swarm-control-N` et
> routage des runs vers l'hôte — voir §3 et §7), et le **mode agent** construit lui aussi :
> exécutable par agent et pont HTTP `--bridge` (§4, `main` 4a2d1cf). **Commités** (`efbe821`) : le champ `executable` de
> `create_agent` (+ `unit`), le journal des tours et les outils `history` / `session` /
> `logs` / `config`. **Dans l'arbre de travail** (non encore commités) : les pairs automatiques.
> Décidée le 2026-11-26 : les agents
> sont **résidents**, et l'architecture accepte **plusieurs instances**.
> Les formats d'annonce (`litemcp-registry`) sont ceux de LiteMCP. Construits : `ask`, `status`,
> `stop`, `agent_start`, `agent_stop`, `--instance`, le mode agent (`--agent` / binaire renommé,
> `--port`, `serve_agent()`) et le pont `--bridge`. **Dans l'arbre de travail** : les pairs
> automatiques ; le champ `executable` et le journal sont commités (`efbe821`).

## 1. Trois objets

| Objet | Ce que c'est | Où |
|---|---|---|
| **Instance** | un process `swarm-control` : un serveur d'administration (11 outils, dont `agent_start`, `agent_stop`) | nom `swarm-control-N`, un port |
| **Agent résident** | un agent *chargé en mémoire* : son propre serveur MCP, sa file de demandes, son fil de travail | nom `swarm-<agent>`, un port |
| **Pair** | un autre agent résident, retrouvé par le registre LiteMCP | `%TEMP%/litemcp-registry/swarm-*.json` |

Un agent persistant a son **propre exécutable compilé** (`swarm-control build alice`, ou
`create_agent {executable:true}`) : son nom et sa classe sont gravés dans le binaire, et lancé
seul il ne sert que l'agent `alice` (mode agent et pont détaillés au §4). Un `swarm-control`
copié ou renommé sous un nom d'agent est **refusé** depuis la 0.2.0 (code 2, message de
migration) ; `swarm-control` seul peut en héberger plusieurs.

```
client MCP ─▶ swarm-control-1 (admin :23080)             swarm-control-2 (admin :23081)
                 héberge  ┌ swarm-alice  :23082              héberge  └ swarm-carol :23084
                          └ swarm-bob    :23083
  le CLI de alice (claude -p) ──MCP──▶ swarm-bob / swarm-carol    (adresses lues dans le registre)
```

Un **archétype** (`run_agent {archetype:"coder"}`) n'est pas un agent résident : c'est un tour
jetable qui porte la persona d'un modèle — ni nom, ni mémoire, ni session gardée. On préfère un
archétype pour une tâche ponctuelle, un agent nommé dès qu'il faut se souvenir, tourner en
continu ou être joint par un pair.

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
   le propriétaire. Les anciens fichiers ne sont plus lus : ce repli, héritage d'OverMind (qui n'est
   plus supporté), a été retiré.
4. **Routage** *(✅ construit)* : `run_agent("alice")` (et chaque tâche de `run_agents_parallel`)
   d'un agent qui a un hôte vivant est **transmis** à cet hôte par `ask` (un `POST /mcp` en `std`,
   module `peers`), même si l'hôte est l'instance qui reçoit l'appel : la file reste l'unique voie.
   La réponse garde la forme de `run_agent` plus `routedTo {instance, url}`. **Reste local, à la
   demande**, exactement comme avant : agent sans hôte, hôte injoignable (connexion refusée avant
   tout envoi), `background:true`, ou run imposant `sessionId`, `path`, `provider` ou
   `autoResume:false` (`ask` reprend toujours la session de l'agent, dans le workspace de l'hôte).
5. **Découverte** : liste des `swarm-*.json` du registre (champs LiteMCP : `name, port, pid, instance,
   exe, args, cwd, ts`) ; le balayeur anti-zombie de LiteMCP retire les annonces mortes.

## 4. Le mode agent et son pont HTTP *(✅ construit — `main` 4a2d1cf)*

Le même binaire, identifié par son **nom de fichier** ou par `--agent NAME`, ne sert qu'**un
seul agent résident** : son serveur MCP (`swarm-<agent>`, outils listés au §5), sa
file, sa session — et **pas de serveur d'administration** (aucun des 11 outils). `--resident` y
est refusé (un agent par process) ; `--port N` fixe le port d'écoute (défaut : premier libre de
23080..=23099), sur le même `--host` que le MCP.

Trois façons équivalentes de l'obtenir :

```bash
cp swarm-control discord && ./discord --port 23090 --bridge 3011 --workspace ~/.swarm-control
swarm-control --agent discord --port 23090 --bridge 3011 --workspace ~/.swarm-control  # sans renommer (unit systemd)
```

```rust
// depuis un crate qui dépend de swarm-control (`fn main() -> ExitCode`) :
swarm_control::serve_agent("discord")
```

`--agent` garde la priorité sur le nom du fichier. La bannière reprend celle d'une instance,
avec le pont en plus :

```text
swarm-discord 0.1.x — MCP sur http://127.0.0.1:23090/mcp  (agent: discord, runner: hermes, workspace: …) · pont HTTP sur http://127.0.0.1:3011
```

### Le pont HTTP `--bridge N`

Il parle le **dialecte Discord** : un routeur Discord existant qui poste déjà `/send` (par exemple
l'ancien `overmind-bridge`, OverMind n'étant plus supporté) n'a qu'à changer de port. HTTP/1.1 minimal en `std`, une requête par connexion, 64 connexions
servies en même temps (au-delà : `503`).

| Route | Requête → réponse |
|---|---|
| `POST /send` | `{message, userId, username, channelId}` → le message devient le bloc `[DISCORD CONTEXT]` habituel, mis dans la **file** de l'agent (un tour à la fois, même session) → `{result, session_id}` ; `result` est **vide** si l'agent a déjà posté sur Discord lui-même via ses outils MCP |
| `POST /reset` | → oublie la session : le prochain message repart à neuf (sûr même pendant un tour) → `{ok, agent, hadSession}` |
| `GET /status` | → état de l'agent, celui de l'outil MCP `status` (idle / busy, file, tour en cours, session) |
| `GET /health` | → sonde de vie : `{status:"ok", agent}` |

`/send` attend la fin du tour jusqu'à une heure ; si le client HTTP se déconnecte, la demande
est annulée tant qu'elle n'a pas démarré ; au-delà du délai : `504`. Un message sans `username`
ni `channelId` est passé brut, hors contexte Discord.

**Jeton partagé** (`SWARM_TOKEN`, lu dans l'environnement ou `.env`) : posé, il protège toutes
les routes sauf `/health` (`Authorization: Bearer …`, comparaison à durée constante) — les
routeurs externes doivent l'envoyer, comme le fait déjà le client `peers` en interne. Sans
jeton, l'écoute reste en **loopback** : toute écoute sur une autre interface est refusée au
démarrage.

## 5. Payloads

### `create_agent` (instance 1, admin)
```json
{"name":"create_agent","arguments":{"name":"alice","runner":"claude","prompt":"Tu es Alice.","resident":true}}
```
```json
{"created":"alice","runner":"claude",
 "executable":"C:\\Users\\Deamon\\.swarm-control\\bin\\alice.exe",   // rendu avec executable:true ; unit avec unit:true (Linux)
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
| `status` | file, tour en cours, session, dernier tour |
| `stop` | tue le tour en cours (arbre de processus ; vide la file si demandé) |
| `history` | les derniers tours, lus dans le journal |
| `session` | voir / oublier / forcer la session |
| `logs` | fin de la sortie du dernier tour |
| `config` | fichiers de configuration (secrets masqués) |

Chaque tour est journalisé par le fil de travail dans
`<workspace>/bridge/history/<runner>-<agent>.jsonl` (une ligne JSON par tour, rotation à
5 Mio) ; le journal nourrit `history`, la CLI et la TUI. Ces quatre outils sont commités
(`efbe821`), comme la CLI d'agent (`serve` par défaut, `ask`, `status`, `history`,
`reset`, `config`, `schema <outil>` ; côté instance : `agents`, `run`, `runs`, `doctor`) et la TUI
(feature `tui`, ratatui + crossterm) — voir PLAN §4.2
et §4.3.

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
La configuration MCP de l'agent regroupe ses serveurs persistants et les pairs vivants du
registre (`agents/peers`, réécrite au démarrage de la résidence et à chaque tour ; un serveur
de l'utilisateur n'est jamais écrasé, trace dans `.swarm-peers.json`) :
```json
{"mcpServers":{"bob":{"url":"http://127.0.0.1:23083/mcp"},
               "carol":{"url":"http://127.0.0.1:23084/mcp"}}}
```
Claude le reçoit par `--mcp-config` (déjà le cas pour ses serveurs) ; Hermes par `mcp_servers` de son
`config.yaml` (déjà écrit par `agents/hermes/mcp.rs`) ; Antigravity par `.agents/mcp_config.json`
du workspace. Son modèle appelle alors `bob.ask(…)` — la clé de serveur est le nom de l'agent,
sans le préfixe `swarm-` de l'annonce (`peers/pairs` insère `peer.agent`) — comme n'importe quel
outil.

## 6. Risques et questions ouvertes

| Risque | Parade prévue |
|---|---|
| **Interblocage** : A demande à B pendant que B demande à A (un seul fil par agent) | chaîne `via` + refus des cycles + `timeoutMs` ; à tester |
| **Durée** : un tour dure des minutes, le client MCP du CLI peut couper avant | `ask` synchrone avec délai, plus une variante asynchrone (`taskId`) — à vérifier sur les CLI réels |
| **Sécurité** : agents en `--dangerously-skip-permissions` qui se pilotent entre eux, sans authentification | loopback seulement ; signature de requêtes LiteMCP (`auth-sign`) avant toute ouverture réseau |
| **Mise à jour du binaire** : un `alice.exe` copié ne suit pas `swarm-control` | liens physiques ou recréation (`swarm-control upgrade`) |
| **Ports** : tranche par défaut = 20 | repli éphémère de LiteMCP ; l'annonce publie le port réel |
| Runners sans configuration MCP injectable (opencode, Kilo, Cline, OpenClaw) | pairs joignables par `ask` côté orchestrateur ; injection au cas par cas, à vérifier runner par runner |

## 7. Étapes testables (de bout en bout, faux CLI d'abord)

1. ✅ **État par agent** + verrou de propriété (sans résidence encore) ; `agents.json` n'est plus
   lu (repli retiré). Construit et testé : 16 acquéreurs simultanés → un seul gagnant (bail frais ou laissé
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
4. *(✅ mode construit, §4 ; fabrication commitée, `efbe821`)* **Exécutable par agent** :
   binaire renommé ou `--agent`, `serve_agent()`, pont `--bridge` (construit dans `main`,
   4a2d1cf). `create_agent {executable:true}` copie le binaire dans `<workspace>/bin/`, rend le
   chemin, `delete_agent` le retire (testé) ; `unit:true` (Linux) écrit en plus une unit
   systemd `--headless`, jamais activée (refusé hors Linux avant toute copie).
5. *(✅ dans l'arbre de travail)* **Pairs** : découverte (`peers/discover`), section des pairs
   vivants, triés par nom d'agent et fusionnés sans écraser les serveurs de l'utilisateur
   (`peers/pairs`, testé), injection dans la configuration MCP de Claude, Hermes et
   Antigravity (`agents/peers`), anti-cycle (testé à l'étape 2).
