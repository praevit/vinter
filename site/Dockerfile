# Vinter website (getvinter.app). Build context: this folder (site/).
#   docker build -t vinter-site site/
FROM nginx:1.30.6-alpine

# Our server config replaces the default site.
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY public/ /usr/share/nginx/html/

# Run as the unprivileged `nginx` user. Docker lets non-root processes bind
# port 80 inside the container (net.ipv4.ip_unprivileged_port_start=0).
# The stock config's `user` directive only applies to root, so drop it, and
# give nginx the few paths it writes to.
RUN sed -i '/^user /d' /etc/nginx/nginx.conf \
    && sed -i 's#^pid .*#pid /tmp/nginx.pid;#' /etc/nginx/nginx.conf \
    && grep -q '^pid /tmp/nginx.pid;' /etc/nginx/nginx.conf \
    && ! grep -q '^user ' /etc/nginx/nginx.conf \
    && chown -R nginx:nginx /var/cache/nginx /usr/share/nginx/html \
    && chmod -R a+rX /usr/share/nginx/html

USER nginx
EXPOSE 80

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1/healthz || exit 1
