//! Find the utun descriptor Network Extension already opened.
//!
//! The iOS SDK does not ship `<sys/kern_control.h>` or `<sys/sys_domain.h>`,
//! so the kernel-control layouts below are declared here. They match the
//! Darwin ABI (`ctl_info` is 100 bytes, `sockaddr_ctl` is 32). Same scan
//! WireGuard uses: `getpeername` on each fd, then `CTLIOCGINFO` for
//! `com.apple.net.utun_control`.

#include "utun_fd.h"

#include <stdint.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/ioccom.h>
#include <sys/socket.h>
#include <unistd.h>

#define XRAYBOX_UTUN_CONTROL_NAME "com.apple.net.utun_control"
#define XRAYBOX_MAX_KCTL_NAME 96
#define XRAYBOX_FD_LIMIT 1024

struct xraybox_ctl_info {
    uint32_t ctl_id;
    char ctl_name[XRAYBOX_MAX_KCTL_NAME];
};

struct xraybox_sockaddr_ctl {
    uint8_t sc_len;
    uint8_t sc_family;
    uint16_t ss_sysaddr;
    uint32_t sc_id;
    uint32_t sc_unit;
    uint32_t sc_reserved[5];
};

_Static_assert(sizeof(struct xraybox_ctl_info) == 100, "ctl_info ABI");
_Static_assert(sizeof(struct xraybox_sockaddr_ctl) == 32, "sockaddr_ctl ABI");

#define XRAYBOX_CTLIOCGINFO _IOWR('N', 3, struct xraybox_ctl_info)

int xraybox_utun_fd(void) {
    struct xraybox_ctl_info info;
    memset(&info, 0, sizeof(info));
    strncpy(info.ctl_name, XRAYBOX_UTUN_CONTROL_NAME, sizeof(info.ctl_name) - 1);

    uint32_t control_id = 0;
    int found = -1;
    uint32_t newest_unit = 0;

    for (int fd = 0; fd < XRAYBOX_FD_LIMIT; fd++) {
        struct xraybox_sockaddr_ctl address;
        memset(&address, 0, sizeof(address));
        socklen_t length = sizeof(address);
        if (getpeername(fd, (struct sockaddr *)&address, &length) != 0) {
            continue;
        }
        if (address.sc_family != AF_SYSTEM) {
            continue;
        }
        if (control_id == 0) {
            if (ioctl(fd, XRAYBOX_CTLIOCGINFO, &info) != 0) {
                continue;
            }
            control_id = info.ctl_id;
        }
        if (address.sc_id != control_id) {
            continue;
        }
        // The interface just created has the highest unit. An older system
        // utun can also be in this table.
        if (found < 0 || address.sc_unit >= newest_unit) {
            found = fd;
            newest_unit = address.sc_unit;
        }
    }
    return found;
}
