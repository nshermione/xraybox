#ifndef XRAYBOX_UTUN_FD_H
#define XRAYBOX_UTUN_FD_H

// The utun descriptor Network Extension opened for this tunnel, or -1.
// Call only after `setTunnelNetworkSettings` has finished.
int xraybox_utun_fd(void);

#endif
