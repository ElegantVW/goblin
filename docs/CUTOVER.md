# Cutover: Purelymail → goblind (vanguardaautomovel.com)

Money goal: cancel Purelymail after this works. Squarespace stays for DNS only.

## Host facts (this machine)

| Item | Value |
|------|--------|
| Public IPv4 | `46.50.107.65` (confirm with `curl -4 ifconfig.me`) |
| LAN | NAT (`192.168.255.49` at last check) — **router must port-forward** |
| Outbound :25 | Works to Gmail MX from this host (probed) |
| Data dir | `GOBLIND_HOME=/home/evenweaker/goblind-data` |
| Binary | `~/.local/bin/goblind` |
| Mail hostname | `mail.vanguardaautomovel.com` |

## Router (required for inbound MX)

Forward to the goblind LAN IP:

| WAN port | LAN target |
|----------|------------|
| 25 | host:25 (inbound SMTP) |
| 465 | host:465 (submission) |
| 993 | host:993 (IMAP) |

Optional: 587 if you prefer STARTTLS submission instead of 465.

## Squarespace DNS (paste by hand)

1. Lower TTL on MX/A if the UI allows (a few hours).
2. **A record:** Host `mail` → `46.50.107.65` (update if IP changes).
3. Run and paste DKIM/SPF/DMARC/MX from:

```bash
export GOBLIND_HOME=/home/evenweaker/goblind-data
goblind sky print-dns vanguardaautomovel.com
```

4. **Order:** publish **A + DKIM + SPF + DMARC (`p=none`)** first; **MX last** (after smoke below).
5. Remove Purelymail MX/SPF/DKIM only after a few clean days.

## Install / start goblind

```bash
cd ~/goblin
git checkout goblind   # or main once merged
cargo build --release --bin goblind --bin goblin
install -m 755 target/release/goblind ~/.local/bin/goblind
install -m 755 target/release/goblin  ~/.local/bin/goblin

export GOBLIND_HOME=/home/evenweaker/goblind-data
goblind user add design@vanguardaautomovel.com   # once
goblind dkim init                                # once
sudo setcap cap_net_bind_service=+ep ~/.local/bin/goblind
sudo cp deploy/goblind.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now goblind
sudo systemctl status goblind
```

Client trust while using lab CA:

```bash
export GOBLIN_EXTRA_CA=$GOBLIND_HOME/tls/ca.pem
```

Point the **Vanguarda** goblin at `mail.vanguardaautomovel.com` (or the public IP) IMAP **993**, SMTP **465**, user `design@vanguardaautomovel.com`, password = goblind user password.

## Smoke checklist (before MX flip)

- [ ] `systemctl is-active goblind`
- [ ] From outside network (phone hotspot): `nc -vz YOUR_PUBLIC_IP 25` and `993`
- [ ] `goblin steal` against goblind
- [ ] `goblin send` to yourself @disroot/gmail — `journalctl -u goblind -f` shows `outbox: … ok`
- [ ] Gmail/disroot “show original” → DKIM **pass** (after DKIM TXT published)
- [ ] Then flip **MX** to `mail.vanguardaautomovel.com`
- [ ] Receive a test from an external account into goblind
- [ ] Export anything needed from Purelymail → cancel Purelymail

## If outbound fails after flip

Port 25 inbound/outbound or IP reputation — move the same `GOBLIND_HOME` to a cheap VPS, update the `mail` A record only. Do not reinstall Purelymail into the stack.

## Office PC later

Stop service → rsync `-aHAX GOBLIND_HOME/` to the office box → install binary + unit → start → update A record if IP differs.
