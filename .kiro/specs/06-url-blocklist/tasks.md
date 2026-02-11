# URL Blocklist + DNS Filtering - Implementation Tasks

## Tasks
- [x] 1. Create `build/dns/Dockerfile` - CoreDNS with blocklist plugin
- [x] 2. Create `config/Corefile` - CoreDNS configuration
- [x] 3. Create `config/dns-blocklist.txt` - DNS blocklist
- [x] 4. Create `config/blocklist.txt` - HTTP blocklist for c-ICAP
- [x] 5. Update `config/c-icap.conf` - Add url_check service
- [x] 6. Update `config/g3proxy.yaml` - Use CoreDNS resolver (10.30.1.10)
- [x] 7. Create `scripts/validate-blocklist.sh` - Validation script
- [x] 8. Update `docker-compose.yml` - Add dns service, update icap and workspace

## Verification Plan
- [ ] Build containers successfully
- [ ] `nslookup webhook.site` from workspace → NXDOMAIN
- [ ] `nslookup github.com` from workspace → resolves
- [x] Empty blocklist → validation fails (exit 1)
- [ ] CoreDNS healthcheck passes
- [ ] ICAP healthcheck passes
