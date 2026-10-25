# Inloggning i debugbygge

Argon2id i en ooptimerad debugbygge gjorde inloggningen långsam: omkring tio sekunder på en Windows-laptop. Dev-profilen bygger paketen `argon2` och `blake2` med `opt-level = 3`. Inloggningen verifierar ett befintligt hash. Mätningen nedan är hash plus verify av ett lösenord i samma profil, ett prov.

- Kommando: `cargo test -p braian-api --lib password_hash_stays_fast -- --nocapture`
- Resultat: 41 ms
- Datum: 2026-10-25
- Maskin: virtuell Linux 6.12, 4 vCPU, `rustc 1.93.1`
- Testet kräver att samma anrop är under 2 sekunder, så en debugbygge utan den profilen faller.

Siffran gäller det här bygget. Den ersätter inte en mätning på den laptop där de tio sekunderna syntes.
