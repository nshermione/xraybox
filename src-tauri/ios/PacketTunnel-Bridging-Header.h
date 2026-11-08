#ifndef XRAYBOX_TUNNEL_BRIDGING_H
#define XRAYBOX_TUNNEL_BRIDGING_H

int xraybox_utun_fd(void);

int hev_socks5_tunnel_main_from_str(const unsigned char *config_str,
                                    unsigned int config_len, int tun_fd);
void hev_socks5_tunnel_quit(void);

int XrayStart(const char *config_path, const char *asset_dir, char *err_buf, int err_len);
void XrayStop(void);
int XrayRunning(void);
unsigned long long XrayProxyUplink(void);
unsigned long long XrayProxyDownlink(void);
unsigned long long XrayGoMemory(void);
unsigned long long XrayGCCycles(void);
unsigned long long XrayGCCPUMillis(void);

#endif
