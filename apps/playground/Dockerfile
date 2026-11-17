# Build from the repository root: docker build -f apps/playground/Dockerfile .
FROM node:24-alpine AS deps
RUN npm install --global pnpm@12.21.0
WORKDIR /repo
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY contracts/package.json contracts/
COPY apps/playground/package.json apps/playground/
RUN pnpm install --frozen-lockfile --prod --filter @capricorn/playground

FROM node:24-alpine
ENV NODE_ENV=production
COPY --from=deps /repo /repo
WORKDIR /repo/apps/playground
COPY apps/playground/src ./src
COPY apps/playground/public ./public
USER node
CMD ["node", "src/server.ts"]
