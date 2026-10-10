# ---- Build stage ----
FROM node:22-alpine AS builder

WORKDIR /app
COPY package.json package-lock.json ./
RUN npm install
COPY . .

ENV NEXT_PUBLIC_API_URL=""
RUN npm run build

# ---- Runtime stage ----
FROM node:22-alpine

WORKDIR /app
RUN addgroup -S lumenpod && adduser -S lumenpod -G lumenpod

COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public

USER lumenpod
EXPOSE 3000

ENV PORT=3000
ENV HOSTNAME=0.0.0.0

CMD ["node", "server.js"]
