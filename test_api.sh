#!/bin/bash

# TeleTable API Test Script
# This script tests all API endpoints

BASE_URL="http://localhost:3000"
TOKEN=""
USER_ID=""
DIARY_ID=""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
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

echo ""
echo "========================================"
echo "  TeleTable API Testing"
echo "========================================"
echo ""

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
REGISTER_RESPONSE=$(curl -s -X POST "$BASE_URL/register" \
    -H "Content-Type: application/json" \
    -d '{
        "name": "Test User",
        "email": "test@example.com",
        "password": "testpassword123"
    }')

if echo "$REGISTER_RESPONSE" | grep -q "id"; then
    echo_success "User registration successful"
    USER_ID=$(echo "$REGISTER_RESPONSE" | grep -o '"id":"[^"]*' | cut -d'"' -f4)
    echo "  User ID: $USER_ID"
else
    echo_error "User registration failed"
    echo "$REGISTER_RESPONSE"
fi

# Test 3: Login
echo_info "Logging in..."
LOGIN_RESPONSE=$(curl -s -X POST "$BASE_URL/login" \
    -H "Content-Type: application/json" \
    -d '{
        "email": "test@example.com",
        "password": "testpassword123"
    }')

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
ME_RESPONSE=$(curl -s "$BASE_URL/me" \
    -H "Authorization: Bearer $TOKEN")

if echo "$ME_RESPONSE" | grep -q "email"; then
    echo_success "Get user info successful"
else
    echo_error "Get user info failed"
    echo "$ME_RESPONSE"
fi

# Test 5: Create diary entry
echo_info "Creating diary entry..."
DIARY_RESPONSE=$(curl -s -X POST "$BASE_URL/diary" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{
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
ALL_DIARY_RESPONSE=$(curl -s "$BASE_URL/diary" \
    -H "Authorization: Bearer $TOKEN")

if echo "$ALL_DIARY_RESPONSE" | grep -q "working_minutes"; then
    echo_success "Retrieved diary entries"
else
    echo_error "Failed to retrieve diary entries"
    echo "$ALL_DIARY_RESPONSE"
fi

# Test 7: Get specific diary entry
if [ ! -z "$DIARY_ID" ]; then
    echo_info "Getting specific diary entry..."
    SPECIFIC_DIARY_RESPONSE=$(curl -s "$BASE_URL/diary?id=$DIARY_ID" \
        -H "Authorization: Bearer $TOKEN")

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
    DELETE_RESPONSE=$(curl -s -X DELETE "$BASE_URL/diary" \
        -H "Authorization: Bearer $TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"id\": \"$DIARY_ID\"}" \
        -w "\n%{http_code}")

    HTTP_CODE=$(echo "$DELETE_RESPONSE" | tail -n 1)
    if [ "$HTTP_CODE" == "204" ]; then
        echo_success "Diary entry deleted"
    else
        echo_error "Failed to delete diary entry (HTTP $HTTP_CODE)"
    fi
fi

# Test 9: Test authentication failure
echo_info "Testing authentication failure..."
UNAUTH_RESPONSE=$(curl -s "$BASE_URL/diary" \
    -w "\n%{http_code}")

HTTP_CODE=$(echo "$UNAUTH_RESPONSE" | tail -n 1)
if [ "$HTTP_CODE" == "401" ]; then
    echo_success "Authentication properly required"
else
    echo_error "Authentication not properly enforced"
fi

echo ""
echo "========================================"
echo "  Testing Complete!"
echo "========================================"
echo ""
