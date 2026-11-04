Whether this folder's own .glut settings count in full.

/trust — whether this folder is trusted, and what its own settings add once it is
/trust on — trusts it: its allow rules, its mode, its skills, agents and commands count
/trust off — trusts it no more: only its deny and ask rules count
/trust list — every trusted folder

A project's .glut/settings.json may keep more secret and ask or refuse more — that counts in every folder. What would let a model do more, its allow rules and its mode, counts only where you trusted the folder, and so do the skills, agents and commands in its .glut. Read what /trust shows before you say on: a cloned repository's .glut is someone else's word. Its mode never goes past your own, and its hooks never run, trusted or not.

The change counts from the folder's next session (glut -c takes the conversation up again). Never the home folder, the root, or a folder that is not yours alone (another user's, or one anybody may write, like /tmp). The list is kept in ~/.glut/trusted.json, never written by a command Glut runs; glut trust show|add|remove|list does the same outside Glut.
