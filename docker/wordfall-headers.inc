# Sent on every response. frame-ancestors is 'self', not 'none', and there is
# no X-Frame-Options: DENY, because the server export downloads through a
# same-origin hidden frame (PLAN.md § Deployment and Operations).
add_header Content-Security-Policy "frame-ancestors 'self'" always;
add_header X-Content-Type-Options "nosniff" always;
add_header Strict-Transport-Security $wordfall_hsts always;
