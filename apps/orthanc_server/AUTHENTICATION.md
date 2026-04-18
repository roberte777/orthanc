# Authentication Architecture

Orthanc uses JWT (JSON Web Token) based authentication for secure, stateless user authentication.

## Overview

The authentication system uses a dual-token approach:
- **Access Tokens**: Short-lived (15-30 minutes), stateless JWTs containing user claims
- **Refresh Tokens**: Long-lived (7-30 days), stored in the database for session management

## Database Tables

### users
Stores user account information:
- `id`: Primary key
- `username`: Unique username
- `email`: Unique email address
- `password_hash`: Bcrypt/Argon2 hashed password
- `display_name`: User's display name
- `is_admin`: Admin privileges flag
- `is_active`: Account enabled/disabled status
- `created_at`, `updated_at`, `last_login_at`: Timestamps

### user_sessions
Tracks active sessions with refresh tokens:
- `id`: Primary key
- `user_id`: Foreign key to users table
- `refresh_token_hash`: SHA256 hash of the refresh token
- `device_name`: Human-readable device identifier (e.g., "Chrome on Windows")
- `device_id`: Unique device identifier for tracking
- `ip_address`: Last known IP address
- `user_agent`: Browser/client user agent string
- `created_at`: When the session was created
- `expires_at`: When the refresh token expires
- `last_used_at`: Last time the refresh token was used
- `is_revoked`: Manual revocation flag (for security)

**Features:**
- Users can view all active sessions
- Users can revoke sessions from specific devices
- Admins can revoke all sessions for a user
- Automatic cleanup of expired sessions

### password_reset_tokens
Manages secure password reset flow:
- `id`: Primary key
- `user_id`: Foreign key to users table
- `token_hash`: SHA256 hash of the reset token
- `expires_at`: Token expiration (typically 1 hour)
- `created_at`: When the token was created
- `used_at`: When the token was used (NULL if unused)

**Security features:**
- Single-use tokens (marked as used after consumption)
- Short expiration time (1 hour recommended)
- Tokens are hashed in the database
- Old tokens automatically cleaned up

## Authentication Flow

### 1. User Registration
```
POST /api/auth/register
{
  "username": "john_doe",
  "email": "john@example.com",
  "password": "secure_password",
  "display_name": "John Doe"
}

Response:
{
  "user": { "id": 1, "username": "john_doe", ... },
  "access_token": "eyJhbGc...",
  "refresh_token": "rand_secure_token",
  "expires_in": 900
}
```

### 2. User Login
```
POST /api/auth/login
{
  "username": "john_doe",
  "password": "secure_password",
  "device_name": "Chrome on MacOS"
}

Response:
{
  "access_token": "eyJhbGc...",
  "refresh_token": "rand_secure_token",
  "expires_in": 900,
  "user": { "id": 1, "username": "john_doe", ... }
}
```

**Process:**
1. Verify username/password against database
2. Generate short-lived JWT access token with user claims
3. Generate cryptographically secure refresh token
4. Hash and store refresh token in `user_sessions` table
5. Return both tokens to client

### 3. Accessing Protected Resources
```
GET /api/media/movies
Authorization: Bearer eyJhbGc...

Response:
{
  "movies": [...]
}
```

**Process:**
1. Client sends access token in Authorization header
2. Server validates JWT signature and expiration
3. Server extracts user ID and permissions from JWT claims
4. Request processed if valid, 401 Unauthorized if invalid/expired

### 4. Refreshing Access Token
```
POST /api/auth/refresh
{
  "refresh_token": "rand_secure_token"
}

Response:
{
  "access_token": "eyJhbGc...",
  "refresh_token": "new_rand_secure_token",
  "expires_in": 900
}
```

**Process:**
1. Hash provided refresh token
2. Look up session in `user_sessions` table
3. Verify session is not expired or revoked
4. Generate new access token
5. **Optional**: Rotate refresh token (generate new one, invalidate old)
6. Update `last_used_at` timestamp
7. Return new tokens

### 5. Logout
```
POST /api/auth/logout
{
  "refresh_token": "rand_secure_token"
}

Response:
{
  "success": true
}
```

**Process:**
1. Mark session as revoked in database
2. **Optional**: Delete session from database
3. Client discards both tokens

### 6. Password Reset Request
```
POST /api/auth/forgot-password
{
  "email": "john@example.com"
}

Response:
{
  "success": true,
  "message": "If an account exists, a reset email has been sent"
}
```

**Process:**
1. Look up user by email
2. Generate secure reset token
3. Hash and store in `password_reset_tokens` table
4. Send email with reset link containing token
5. Always return success (prevent email enumeration)

### 7. Password Reset
```
POST /api/auth/reset-password
{
  "token": "reset_token_from_email",
  "new_password": "new_secure_password"
}

Response:
{
  "success": true
}
```

**Process:**
1. Hash provided token and look up in database
2. Verify token exists, not expired, and not used
3. Update user's password hash
4. Mark token as used (`used_at` timestamp)
5. **Optional**: Revoke all active sessions for security
6. Return success

### 8. Session Management
```
GET /api/auth/sessions

Response:
{
  "sessions": [
    {
      "id": 1,
      "device_name": "Chrome on MacOS",
      "ip_address": "192.168.1.100",
      "created_at": "2026-04-17T...",
      "last_used_at": "2026-04-18T...",
      "current": true
    },
    {
      "id": 2,
      "device_name": "Safari on iPhone",
      "ip_address": "10.0.0.50",
      "created_at": "2026-04-15T...",
      "last_used_at": "2026-04-17T...",
      "current": false
    }
  ]
}

DELETE /api/auth/sessions/2

Response:
{
  "success": true
}
```

## JWT Structure

### Access Token Claims
```json
{
  "sub": "1",                    // User ID
  "username": "john_doe",
  "email": "john@example.com",
  "is_admin": false,
  "iat": 1713398400,            // Issued at
  "exp": 1713399300             // Expires (15 min later)
}
```

### Token Configuration
- **Algorithm**: HS256 (HMAC with SHA-256) or RS256 (RSA with SHA-256)
- **Access Token Lifetime**: 15-30 minutes
- **Refresh Token Lifetime**: 7-30 days
- **Secret Key**: Stored in environment variable `JWT_SECRET`

## Security Considerations

### Password Storage
- Use **Argon2id** or **bcrypt** for password hashing
- Minimum cost factor: 12 for bcrypt, recommended Argon2id params
- Never store plaintext passwords

### Token Security
- **Never** store access tokens in the database (stateless)
- Store **hashed** refresh tokens only (SHA256)
- Use cryptographically secure random generators for tokens
- Implement token rotation on refresh for added security

### Rate Limiting
- Login attempts: 5 per 15 minutes per IP
- Password reset requests: 3 per hour per email
- Token refresh: 10 per minute per session

### Additional Security
- HTTPS required in production
- Secure, HttpOnly cookies for refresh tokens (web clients)
- CORS properly configured
- CSRF protection for cookie-based auth
- Account lockout after failed login attempts
- Email verification for new accounts (optional)

## Environment Variables

```bash
# JWT Configuration
JWT_SECRET=your_secret_key_min_32_chars
JWT_ACCESS_TOKEN_EXPIRY=900        # 15 minutes in seconds
JWT_REFRESH_TOKEN_EXPIRY=2592000   # 30 days in seconds

# Session Configuration
SESSION_MAX_DEVICES=10             # Max simultaneous sessions per user
SESSION_CLEANUP_INTERVAL=3600      # Cleanup expired sessions every hour

# Email Configuration (for password reset)
SMTP_HOST=smtp.gmail.com
SMTP_PORT=587
SMTP_USER=noreply@orthanc.example.com
SMTP_PASSWORD=app_specific_password
PASSWORD_RESET_URL=https://orthanc.example.com/reset-password
```

## Implementation Checklist

When implementing JWT authentication:

- [ ] Add JWT dependencies (jsonwebtoken, argon2, rand)
- [ ] Create user registration endpoint
- [ ] Create login endpoint with password verification
- [ ] Implement JWT token generation
- [ ] Create refresh token endpoint with rotation
- [ ] Add authentication middleware to protect routes
- [ ] Implement logout endpoint
- [ ] Create password reset request endpoint
- [ ] Create password reset confirmation endpoint
- [ ] Add session management endpoints
- [ ] Implement automatic session cleanup job
- [ ] Add rate limiting to auth endpoints
- [ ] Configure CORS and security headers
- [ ] Set up email service for password resets
- [ ] Add account lockout after failed attempts
- [ ] Write comprehensive auth tests

## Future Enhancements

- **OAuth2/OIDC**: Support for Google, GitHub, etc.
- **2FA/MFA**: TOTP-based two-factor authentication
- **WebAuthn**: Passwordless authentication with security keys
- **Magic Links**: Email-based passwordless login
- **SSO**: Single Sign-On integration for enterprise
