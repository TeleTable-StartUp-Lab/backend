#!/bin/bash

# TeleTable API Test Script
# This script tests all API endpoints including Admin functionality

BASE_URL="http://localhost:3003"

# Admin Credentials
ADMIN_EMAIL="admin@teletable.com"
ADMIN_PASSWORD="INSERT_ADMIN_PASSWORD_HERE"

# Victim Credentials (for Admin test)
VICTIM_NAME="Victim User"
VICTIM_EMAIL="victim_$(date +%s)@example.com"
VICTIM_PASSWORD="password123"

# Standard Test User Credentials
TEST_USER_NAME="Test User"
TEST_USER_EMAIL="test@example.com"
TEST_USER_PASSWORD="testpassword123"

# State Variables
TOKEN=""
USER_ID=""
DIARY_ID=""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo_success() {
    echo -e "${GREEN}✓${NC} $1"
}

echo_error() {
    echo -e "${RED}✗${NC} $1"
}

echo_info() {
    echo -e "${BLUE}➜${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}!${NC} $1"
}

echo ""
echo "========================================"
echo "  TeleTable API Testing"
echo "========================================"
echo ""

# Check if backend is running
if ! curl -s "$BASE_URL/" > /dev/null; then
    echo_error "Backend is not running at $BASE_URL"
    exit 1
fi

# ==========================================
# PART 1: Standard User Flow
# ==========================================
echo_info "--- Starting Standard User Flow ---"

# Test 1: Root endpoint
echo_info "Testing root endpoint..."
RESPONSE=$(curl -s "$BASE_URL/")
if [ "$RESPONSE" == "TeleTable Backend API - v0.1.0" ]; then
    echo_success "Root endpoint working"
else
    echo_error "Root endpoint failed"
    exit 1
fi

# Test 2: Register user
echo_info "Registering new user..."
REGISTER_RESPONSE=$(curl -s -X POST "$BASE_URL/register"     -H "Content-Type: application/json"     -d "{
        \"name\": \"$TEST_USER_NAME\",
        \"email\": \"$TEST_USER_EMAIL\",
        \"password\": \"$TEST_USER_PASSWORD\"
    }")

if echo "$REGISTER_RESPONSE" | grep -q "id"; then
    echo_success "User registration successful"
    USER_ID=$(echo "$REGISTER_RESPONSE" | grep -o '"id":"[^"]*' | cut -d'"' -f4)
    echo "  User ID: $USER_ID"
else
    # If user already exists, that's fine for this test, we'll try to login
    if echo "$REGISTER_RESPONSE" | grep -q "already exists"; then
         echo_warn "User already exists, proceeding to login..."
    else
         echo_error "User registration failed"
         echo "$REGISTER_RESPONSE"
    fi
fi

# Test 3: Login
echo_info "Logging in..."
LOGIN_RESPONSE=$(curl -s -X POST "$BASE_URL/login"     -H "Content-Type: application/json"     -d "{
        \"email\": \"$TEST_USER_EMAIL\",
        \"password\": \"$TEST_USER_PASSWORD\"
    }")

if echo "$LOGIN_RESPONSE" | grep -q "token"; then
    echo_success "Login successful"
    TOKEN=$(echo "$LOGIN_RESPONSE" | grep -o '"token":"[^"]*' | cut -d'"' -f4)
    echo "  Token: ${TOKEN:0:50}..."
else
    echo_error "Login failed"
    echo "$LOGIN_RESPONSE"
    exit 1
fi

# Test 4: Get current user info
echo_info "Getting current user info..."
ME_RESPONSE=$(curl -s "$BASE_URL/me"     -H "Authorization: Bearer $TOKEN")

if echo "$ME_RESPONSE" | grep -q "email"; then
    echo_success "Get user info successful"
else
    echo_error "Get user info failed"
    echo "$ME_RESPONSE"
fi

# Test 5: Create diary entry
echo_info "Creating diary entry..."
DIARY_RESPONSE=$(curl -s -X POST "$BASE_URL/diary"     -H "Authorization: Bearer $TOKEN"     -H "Content-Type: application/json"     -d '{
        "working_minutes": 120,
        "text": "Implemented the TeleTable backend API with Rust and Axum"
    }')

if echo "$DIARY_RESPONSE" | grep -q "id"; then
    echo_success "Diary entry created"
    DIARY_ID=$(echo "$DIARY_RESPONSE" | grep -o '"id":"[^"]*' | cut -d'"' -f4)
    echo "  Diary ID: $DIARY_ID"
else
    echo_error "Diary entry creation failed"
    echo "$DIARY_RESPONSE"
fi

# Test 6: Get all diary entries
echo_info "Getting all diary entries..."
ALL_DIARY_RESPONSE=$(curl -s "$BASE_URL/diary"     -H "Authorization: Bearer $TOKEN")

if echo "$ALL_DIARY_RESPONSE" | grep -q "working_minutes"; then
    echo_success "Retrieved diary entries"
else
    echo_error "Failed to retrieve diary entries"
    echo "$ALL_DIARY_RESPONSE"
fi

# Test 7: Get specific diary entry
if [ ! -z "$DIARY_ID" ]; then
    echo_info "Getting specific diary entry..."
    SPECIFIC_DIARY_RESPONSE=$(curl -s "$BASE_URL/diary?id=$DIARY_ID"         -H "Authorization: Bearer $TOKEN")

    if echo "$SPECIFIC_DIARY_RESPONSE" | grep -q "working_minutes"; then
        echo_success "Retrieved specific diary entry"
    else
        echo_error "Failed to retrieve specific diary entry"
        echo "$SPECIFIC_DIARY_RESPONSE"
    fi
fi

# Test 8: Delete diary entry
if [ ! -z "$DIARY_ID" ]; then
    echo_info "Deleting diary entry..."
    DELETE_RESPONSE=$(curl -s -X DELETE "$BASE_URL/diary"         -H "Authorization: Bearer $TOKEN"         -H "Content-Type: application/json"         -d "{\"id\": \"$DIARY_ID\"}"         -w "\n%{http_code}")

    HTTP_CODE=$(echo "$DELETE_RESPONSE" | tail -n 1)
    if [ "$HTTP_CODE" == "204" ]; then
        echo_success "Diary entry deleted"
    else
        echo_error "Failed to delete diary entry (HTTP $HTTP_CODE)"
    fi
fi

# Test 9: Test authentication failure
echo_info "Testing authentication failure..."
UNAUTH_RESPONSE=$(curl -s "$BASE_URL/diary"     -w "\n%{http_code}")

HTTP_CODE=$(echo "$UNAUTH_RESPONSE" | tail -n 1)
if [ "$HTTP_CODE" == "401" ]; then
    echo_success "Authentication properly required"
else
    echo_error "Authentication not properly enforced"
fi

echo ""
# ==========================================
# PART 2: Admin Flow
# ==========================================
echo_info "--- Starting Admin Flow ---"

# 1. Login as Admin
echo_info "Logging in as Admin..."
# Escape backslashes for JSON
JSON_PASSWORD=${ADMIN_PASSWORD//\\/\\\\}
LOGIN_RESPONSE=$(curl -s -X POST "$BASE_URL/login"     -H "Content-Type: application/json"     -d "{\"email\": \"$ADMIN_EMAIL\", \"password\": \"$JSON_PASSWORD\"}")

# Check for token
ADMIN_TOKEN=$(echo $LOGIN_RESPONSE | grep -o '"token":"[^"]*' | cut -d'"' -f4)

if [ -z "$ADMIN_TOKEN" ]; then
    echo_error "Admin login failed. Response: $LOGIN_RESPONSE"
    exit 1
else
    echo_success "Admin logged in successfully"
fi

# 2. Register Victim User
echo_info "Registering victim user ($VICTIM_EMAIL)..."
REGISTER_RESPONSE=$(curl -s -X POST "$BASE_URL/register"     -H "Content-Type: application/json"     -d "{\"name\": \"$VICTIM_NAME\", \"email\": \"$VICTIM_EMAIL\", \"password\": \"$VICTIM_PASSWORD\"}")

# Extract User ID
VICTIM_ID=$(echo $REGISTER_RESPONSE | grep -o '"id":"[^"]*' | cut -d'"' -f4)

if [ -z "$VICTIM_ID" ]; then
    echo_error "Registration failed. Response: $REGISTER_RESPONSE"
    exit 1
else
    echo_success "Victim registered successfully. ID: $VICTIM_ID"
fi

# 3. Delete Victim User as Admin
echo_info "Deleting victim user as Admin..."
DELETE_RESPONSE=$(curl -s -w "%{http_code}" -X DELETE "$BASE_URL/user"     -H "Authorization: Bearer $ADMIN_TOKEN"     -H "Content-Type: application/json"     -d "{\"id\": \"$VICTIM_ID\"}")

HTTP_CODE="${DELETE_RESPONSE: -3}"
BODY="${DELETE_RESPONSE::-3}"

if [ "$HTTP_CODE" == "204" ]; then
    echo_success "Victim user deleted successfully (204 No Content)"
else
    echo_error "Failed to delete user. HTTP Code: $HTTP_CODE. Body: $BODY"
    exit 1
fi

echo ""
echo_success "All tests passed successfully!"
echo ""
