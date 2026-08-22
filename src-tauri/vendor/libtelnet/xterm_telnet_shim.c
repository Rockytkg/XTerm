#include "libtelnet.h"
#include <stddef.h>

typedef void (*xterm_telnet_callback_t)(
    void *user_data,
    int kind,
    unsigned char command,
    unsigned char option,
    const char *buffer,
    size_t size,
    int error_code,
    const char *message);

enum {
    XTERM_TELNET_DATA = 0,
    XTERM_TELNET_SEND = 1,
    XTERM_TELNET_IAC = 2,
    XTERM_TELNET_WILL = 3,
    XTERM_TELNET_WONT = 4,
    XTERM_TELNET_DO = 5,
    XTERM_TELNET_DONT = 6,
    XTERM_TELNET_SUBNEGOTIATION = 7,
    XTERM_TELNET_WARNING = 8,
    XTERM_TELNET_ERROR = 9
};

static const telnet_telopt_t XTERM_TELOPTS[] = {
    { TELNET_TELOPT_BINARY,      TELNET_WILL, TELNET_DO },
    { TELNET_TELOPT_ECHO,        TELNET_WONT, TELNET_DO },
    { TELNET_TELOPT_SGA,         TELNET_WILL, TELNET_DO },
    { TELNET_TELOPT_TTYPE,       TELNET_WILL, TELNET_DONT },
    { TELNET_TELOPT_NAWS,        TELNET_WILL, TELNET_DONT },
    { TELNET_TELOPT_NEW_ENVIRON, TELNET_WILL, TELNET_DONT },
    { -1, 0, 0 }
};

struct xterm_telnet_context {
    xterm_telnet_callback_t callback;
    void *user_data;
};

static void xterm_telnet_event(
    telnet_t *telnet,
    telnet_event_t *event,
    void *opaque) {
    struct xterm_telnet_context *context = opaque;
    (void)telnet;
    switch (event->type) {
    case TELNET_EV_DATA:
        context->callback(context->user_data, XTERM_TELNET_DATA, 0, 0,
            event->data.buffer, event->data.size, 0, NULL);
        break;
    case TELNET_EV_SEND:
        context->callback(context->user_data, XTERM_TELNET_SEND, 0, 0,
            event->data.buffer, event->data.size, 0, NULL);
        break;
    case TELNET_EV_IAC:
        context->callback(context->user_data, XTERM_TELNET_IAC,
            event->iac.cmd, 0, NULL, 0, 0, NULL);
        break;
    case TELNET_EV_WILL:
    case TELNET_EV_WONT:
    case TELNET_EV_DO:
    case TELNET_EV_DONT:
        context->callback(context->user_data,
            event->type == TELNET_EV_WILL ? XTERM_TELNET_WILL :
            event->type == TELNET_EV_WONT ? XTERM_TELNET_WONT :
            event->type == TELNET_EV_DO ? XTERM_TELNET_DO : XTERM_TELNET_DONT,
            0, event->neg.telopt, NULL, 0, 0, NULL);
        break;
    case TELNET_EV_SUBNEGOTIATION:
        context->callback(context->user_data, XTERM_TELNET_SUBNEGOTIATION,
            0, event->sub.telopt, event->sub.buffer, event->sub.size, 0, NULL);
        break;
    case TELNET_EV_WARNING:
    case TELNET_EV_ERROR:
        context->callback(context->user_data,
            event->type == TELNET_EV_WARNING ? XTERM_TELNET_WARNING : XTERM_TELNET_ERROR,
            0, 0, NULL, 0, event->error.errcode, event->error.msg);
        break;
    default:
        break;
    }
}

telnet_t *xterm_telnet_init(
    struct xterm_telnet_context *context,
    xterm_telnet_callback_t callback,
    void *user_data) {
    context->callback = callback;
    context->user_data = user_data;
    return telnet_init(XTERM_TELOPTS, xterm_telnet_event, 0, context);
}

void xterm_telnet_free(telnet_t *telnet) { telnet_free(telnet); }
void xterm_telnet_recv(telnet_t *telnet, const char *data, size_t size) {
    telnet_recv(telnet, data, size);
}
void xterm_telnet_negotiate(telnet_t *telnet, unsigned char command, unsigned char option) {
    telnet_negotiate(telnet, command, option);
}
void xterm_telnet_send_text(telnet_t *telnet, const char *data, size_t size) {
    telnet_send_text(telnet, data, size);
}
void xterm_telnet_subnegotiation(
    telnet_t *telnet,
    unsigned char option,
    const char *data,
    size_t size) {
    telnet_subnegotiation(telnet, option, data, size);
}
