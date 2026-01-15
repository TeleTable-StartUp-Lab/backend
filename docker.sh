#!/bin/bash

# TeleTable Backend - Docker Compose Helper Script

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_env_file() {
    if [ ! -f .env ]; then
        print_warning ".env file not found. Copying from .env.example..."
        cp .env.example .env
        print_warning "Please edit .env file with your secure credentials before proceeding!"
        exit 1
    fi
}

dev_start() {
    print_info "Starting development environment..."
    check_env_file
    docker compose up -d
    print_info "Services started. Backend available at http://localhost:3003"
    print_info "View logs with: $0 logs"
}

dev_stop() {
    print_info "Stopping development environment..."
    docker compose down
}

dev_restart() {
    print_info "Restarting development environment..."
    docker compose restart
}

prod_start() {
    print_info "Starting production environment..."
    check_env_file
    docker compose -f docker-compose.prod.yml up -d
    print_info "Production services started."
}

prod_stop() {
    print_info "Stopping production environment..."
    docker compose -f docker-compose.prod.yml down
}

show_logs() {
    docker compose logs -f "${@:1}"
}

build() {
    print_info "Building Docker images..."
    docker compose build
}

clean() {
    print_info "Cleaning up Docker resources..."
    docker compose down -v
    print_warning "This removed all volumes including database data!"
}

shell() {
    print_info "Opening shell in backend container..."
    docker compose exec backend /bin/bash
}

db_shell() {
    print_info "Opening PostgreSQL shell..."
    docker compose exec postgres psql -U teletable -d teletable_db
}

show_help() {
    cat << EOF
TeleTable Backend - Docker Helper Script

Usage: $0 <command>

Commands:
  dev:start     Start development environment
  dev:stop      Stop development environment
  dev:restart   Restart development environment
  
  prod:start    Start production environment
  prod:stop     Stop production environment
  
  logs [service] Show logs (optionally for specific service)
  build         Build Docker images
  clean         Remove all containers and volumes
  shell         Open shell in backend container
  db:shell      Open PostgreSQL shell
  
  help          Show this help message

Examples:
  $0 dev:start
  $0 logs backend
  $0 db:shell

EOF
}

# Main command dispatcher
case "$1" in
    dev:start)
        dev_start
        ;;
    dev:stop)
        dev_stop
        ;;
    dev:restart)
        dev_restart
        ;;
    prod:start)
        prod_start
        ;;
    prod:stop)
        prod_stop
        ;;
    logs)
        show_logs "${@:2}"
        ;;
    build)
        build
        ;;
    clean)
        clean
        ;;
    shell)
        shell
        ;;
    db:shell)
        db_shell
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        print_error "Unknown command: $1"
        show_help
        exit 1
        ;;
esac
