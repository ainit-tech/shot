# The documentation site in development mode, started with `docker compose up -d` from the
# repository root (see tools/docs/README.md). The image holds only the tools: the repository
# is mounted at /repo, so the site shows the docs as they are on disk and rebuilds on changes.
FROM rust:1.95-slim-bookworm

COPY --from=node:24-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=node:24-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules

# The engine reads versions and page history with the git command. The mounted repository
# belongs to another user, so git has to be told to trust it.
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
 && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
 && apt-get update \
 && apt-get install -y --no-install-recommends git \
 && rm -rf /var/lib/apt/lists/* \
 && git config --system --add safe.directory '*'

# git must not lock or rewrite the host's index. File changes from a Windows or macOS host
# don't reach the container as events, so the site watches the files by polling them.
ENV GIT_OPTIONAL_LOCKS=0 \
    DOCS_POLL=1

WORKDIR /repo/tools/docs/ui
EXPOSE 5176
CMD ["sh", "-c", "npm ci --no-audit --no-fund && npm run dev -- --host 0.0.0.0"]
